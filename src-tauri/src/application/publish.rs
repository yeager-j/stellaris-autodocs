//! The Phase 3 build coordination step: obtain a Revision Candidate and publish it.
//!
//! # What this is, and what it is deliberately not
//!
//! The design's build use cases are Ensure Documentation and Rebuild Documentation, which
//! "share one private coordinator with an explicit cache policy" and together derive source
//! identity, establish a Source Snapshot, produce an Analysis Draft, materialize assets,
//! finalize a candidate, and publish (docs/technical-design.md, "Documentation build use
//! cases"). None of that exists yet, and this function is named for what it actually does:
//! it publishes whatever the [`RevisionCandidateSource`] hands over. It is not `ensure` — it
//! consults no cache — and it is not `rebuild` — there is nothing to rebuild from. Calling it
//! either would be a name that lies (AGENTS.md, "Names tell the truth").
//!
//! What it is missing, and where Phase 9 inserts it:
//!
//! - **No cache policy.** Ensure's authoritative freshness check has no fingerprints to check.
//! - **No host-owned build lease.** [`BuildGuard`] is a far smaller thing; see its own note.
//! - **No Source Snapshot**, and therefore
//! - **no pre-publication live-source re-verification.** `revisions` states plainly that
//!   verifying the live source immediately before publishing "is the build coordinator's step,
//!   and `revisions` does not silently acquire it" (D-085). This coordinator does not perform
//!   it either, and the reason is that a hand-authored candidate has no
//!   [`LiveSource`](crate::source::snapshot::LiveSource) behind it — inventing one would feed a
//!   fabricated source observation into a real fingerprint, which is worse than the gap. Phase
//!   9's step goes **between** [`RevisionCandidateSource::candidate_for`] and
//!   [`RevisionStore::publish`] below, and `SourceChangedDuringBuild` joins the error union
//!   there.
//!
//! # Why a guard, when the error union is what it protects
//!
//! `revisions` never reads the publication pointer, so this coordinator reads it, and it reads
//! it *outside* [`RevisionStore`]'s own lock — that lock serializes the protocol's I/O and is
//! explicitly "not the build lease". Two overlapping calls would therefore both observe the
//! same prior revision, serialize inside `publish`, and the loser's compare-and-swap would fail
//! with `ExpectedMismatch`. That outcome is reachable from an ordinary double-click, so
//! reporting it as an invariant violation would make the unexpected channel a lie.
//!
//! [`BuildGuard`] closes it at the only place that can: it spans read-pointer through publish,
//! so within one process a second call is refused up front with
//! [`BuildInProgress`](PublishDocumentationError::BuildInProgress) — the design's own answer,
//! reported "before establishing a Source Snapshot" rather than after work is wasted — and
//! `ExpectedMismatch` becomes genuinely unreachable, which is what earns it the unexpected
//! channel below. One Desktop Host owns one application-data directory (D-065), so "within one
//! process" is the whole of the exposure.
//!
//! # Diagnostic detail is discarded here, not redacted
//!
//! Expected refusals carry a Mod Installation identifier and a machine-readable reason and
//! nothing else, because a payload "does not expose absolute paths or raw framework errors"
//! (docs/technical-design.md, "Serializable result contract"). [`StageError`] names a staging
//! directory, [`MutationError::StorageFailed`] carries replacement detail, and a
//! [`ValidationReport`] names bundle-relative entries — all of it stops here. **This crate has
//! no logging sink**, so that detail is currently dropped rather than routed to a protected
//! log; when one lands, this is the boundary that grows the call, not the payloads.
//!
//! [`StageError`]: crate::revisions::StageError
//! [`ValidationReport`]: crate::revisions::ValidationReport

use crate::application::candidates::{CandidateUnavailable, RevisionCandidateSource};
use crate::application::target::BuildTarget;
use crate::discovery::identity::ModInstallationId;
use crate::error::{Failure, OpResult, Unexpected};
use crate::revisions::{BundleDurability, PointerCommit, PublishError, Published, RevisionStore};
use crate::state::{
    MutationError, PublicationCapability, PublicationError, RevisionId, StateStore,
};
use std::fmt;
use std::sync::{Mutex, PoisonError};

/// Excludes a second concurrent publication **within this process**, and nothing else.
///
/// **Not the Phase 9 build lease, and it must not be mistaken for one** — the same warning
/// [`RevisionStore`] carries about its own mutex. It covers no Source Snapshot establishment,
/// no analysis, no asset materialization, no cancellation, and no cross-installation policy; it
/// is not held across anything but this one function; and holding it grants no right to produce
/// documentation. What it buys is exactly one thing: that the pointer this coordinator reads is
/// still the pointer it publishes against.
///
/// It records *which* installation is active rather than merely that something is, because the
/// design's `BuildInProgress` result carries the active Mod Installation identifier.
#[derive(Default)]
pub struct BuildGuard {
    active: Mutex<Option<ModInstallationId>>,
}

impl BuildGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks `installation` as the active build, or reports the one already running.
    ///
    /// The lock is held only across this inspection, never across the publication itself: a
    /// second caller must be *refused* rather than queued, because "a concurrent Ensure or
    /// Rebuild request ... does not join the existing work, enqueue a hidden follow-up build,
    /// cancel it, or start competing source and asset work".
    ///
    /// Poisoning is recovered from rather than propagated, on [`RevisionStore::lock`]'s
    /// grounds: the guarded value is one `Option`, a panic elsewhere leaves nothing torn in it,
    /// and refusing the lock for the process lifetime would turn an unrelated panic into
    /// permanent inability to publish.
    fn hold(&self, installation: ModInstallationId) -> Result<BuildHold<'_>, ModInstallationId> {
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        match *active {
            Some(current) => Err(current),
            None => {
                *active = Some(installation);
                Ok(BuildHold { guard: self })
            }
        }
    }
}

/// Releases the guard from its own `drop`, so every path out of the coordinator — including
/// `?` on a refusal — clears it.
struct BuildHold<'a> {
    guard: &'a BuildGuard,
}

impl Drop for BuildHold<'_> {
    fn drop(&mut self) {
        *self
            .guard
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// What a completed publication established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentationPublished {
    pub installation: ModInstallationId,
    pub revision: RevisionId,
    pub durability: PublishedDurability,
}

/// What is known about the durability of each of publication's two commit points.
///
/// **Both, separately, rather than one summary.** An earlier version of this type reported only
/// the bundle's flush and called the result `Confirmed` — which claimed the whole publication
/// was durable while the pointer commit's own durability was unknown. Whichever half is
/// unconfirmed, a crash in the seconds around the publication can undo it, so a summary would
/// have to pick a precedence between two facts that are not ordered; two fields invent nothing
/// and map one-to-one from [`Published`](crate::revisions::Published).
///
/// [`PointerCommit`](crate::revisions::PointerCommit) is still not carried *as itself*: its
/// retirement-eligibility consequence belongs to Phase 9's retention sweep, and a workflow
/// reporting to a person needs the durability fact rather than the sweep's input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishedDurability {
    pub bundle_entry: BundleEntryDurability,
    pub publication_record: PublicationRecordDurability,
}

impl PublishedDurability {
    /// Whether every commit point this publication made was confirmed durable.
    ///
    /// The one derived answer worth naming here, so a caller asking the simple question cannot
    /// get it wrong by reading one field and forgetting the other.
    pub fn is_fully_confirmed(&self) -> bool {
        self.bundle_entry == BundleEntryDurability::Flushed
            && self.publication_record == PublicationRecordDurability::Flushed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleEntryDurability {
    Flushed,
    /// This volume provides no way to prove the bundle's directory entry reached disk, so a
    /// crash could still lose it, leaving the revision absent rather than damaged. A permanent
    /// property of the volume, not of this build (D-123).
    NotProvidedByPlatform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationRecordDurability {
    Flushed,
    /// The new revision is what a reader sees now — `state` reopened and validated the
    /// authoritative path — and the replacement's durability was not confirmed, so a crash can
    /// still revert the record to the revision it replaced.
    NotConfirmed,
}

/// Every way this operation can refuse.
///
/// **Four of the design's six build variants are absent, and each absence is a fact about this
/// phase rather than an oversight** (D-081: an operation exposes only what it can produce).
/// `SourceChangedDuringBuild` and `SourceUnavailable` need a Source Snapshot, which no
/// hand-authored candidate has. The two that are here for reasons worth stating:
/// `BuildInProgress` exists only because [`BuildGuard`] can produce it, and
/// `InstallationUnavailable` is checked up front rather than discovered at the pointer commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishDocumentationError {
    /// Another publication is already running in this process. Names the installation it is
    /// running for, which may be a different one.
    BuildInProgress { installation: ModInstallationId },
    /// The Discovery Location this installation was derived from is not configured, so no
    /// publication reference could be stored for it.
    ///
    /// **Checked before any bundle is written**, and that ordering is the point rather than a
    /// tidiness: the state document answers it in one snapshot read, while the pointer commit
    /// would not raise it until commit point 1 had already moved a complete bundle onto its
    /// final path and flushed it — a whole unreferenced revision written for a condition that
    /// was knowable at the start. The check leaves a race (a location removed between check and
    /// commit) which the late path reports as this same variant, so the two agree.
    InstallationUnavailable { installation: ModInstallationId },
    /// No candidate could be produced, so there is nothing to publish.
    AnalysisFailed {
        installation: ModInstallationId,
        reason: AnalysisFailure,
    },
    /// The revisions location or the state document could not be written. Nothing is
    /// published and the previously published revision, if any, is untouched.
    StorageUnavailable {
        installation: ModInstallationId,
        reason: StorageFailure,
    },
}

/// Why no candidate exists. Separate from [`CandidateUnavailable`] because that is the seam's
/// vocabulary and this is the operation's: the design's `AnalysisFailed` is glossed as an
/// "actionable external or environmental condition", and "this build ships no analyzer" is
/// neither. Naming the phase truth in the payload keeps the variant honest rather than
/// stretching its gloss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisFailure {
    NoAnalysisInThisBuild,
    NothingDocumentedForThisInstallation,
}

/// Which step could not write, at the granularity a person can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageFailure {
    /// The staged tree could not be written or flushed. Nothing was moved.
    RevisionNotStaged,
    /// The move to the final path did not complete. Nothing is published.
    RevisionNotCompleted,
    /// The bundle reached its final path and its directory entry could not be confirmed
    /// durable, so the pointer commit was refused (D-121). An identical retry re-flushes it.
    RevisionDurabilityUnconfirmed,
    /// Something that is not this revision's bundle occupies its path, and was left untouched.
    /// Discovered corruption is data rather than a defect — the same stance Validate Published
    /// Revision takes — so it is reported rather than thrown.
    RevisionsLocationUnusable,
    /// The bundle is complete at its final path and the state document could not be replaced,
    /// so the previously published revision stays published.
    StateNotWritten,
    /// State recovery is outstanding, so no mutation is accepted until it is resolved.
    StateRecoveryRequired,
}

impl fmt::Display for PublishDocumentationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuildInProgress { installation } => write!(
                f,
                "documentation is already being built for installation {installation}, so this \
                 request was not started"
            ),
            Self::InstallationUnavailable { installation } => write!(
                f,
                "installation {installation} belongs to a Discovery Location that is not \
                 configured, so its documentation cannot be published"
            ),
            Self::AnalysisFailed {
                installation,
                reason,
            } => write!(
                f,
                "no documentation could be produced for installation {installation}: {reason}"
            ),
            Self::StorageUnavailable {
                installation,
                reason,
            } => write!(
                f,
                "documentation for installation {installation} was not published: {reason}"
            ),
        }
    }
}

impl fmt::Display for AnalysisFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAnalysisInThisBuild => {
                f.write_str("this build performs no documentation analysis")
            }
            Self::NothingDocumentedForThisInstallation => {
                f.write_str("nothing was documented for this mod installation")
            }
        }
    }
}

impl fmt::Display for StorageFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::RevisionNotStaged => "the revision could not be written",
            Self::RevisionNotCompleted => "the revision could not be completed",
            Self::RevisionDurabilityUnconfirmed => {
                "the revision was written and its durability could not be confirmed"
            }
            Self::RevisionsLocationUnusable => {
                "another revision directory occupies this revision's place and was left \
                 untouched"
            }
            Self::StateNotWritten => {
                "the revision was written and the published-revision record could not be updated"
            }
            Self::StateRecoveryRequired => {
                "application state must be recovered before anything further is published"
            }
        };
        f.write_str(text)
    }
}

impl std::error::Error for PublishDocumentationError {}
impl std::error::Error for AnalysisFailure {}
impl std::error::Error for StorageFailure {}

/// Publishes the candidate `candidates` supplies for `target`.
///
/// The steps, in the order the guard makes meaningful:
///
/// ```text
/// 1  take the build guard                    — refuses a concurrent publication
/// 2  read state once: is the location configured, and what is published now
/// 3  ask the source for a candidate
/// 4  check the candidate documents the target                (Phase 9 inserts live-source
/// 5  publish through `revisions`                              verification between 4 and 5)
/// ```
///
/// Steps 2 and 5 both touch `state`, and only step 5 mutates it — through
/// [`PublicationCapability`], which is the whole of `state` that publication is given.
pub fn publish_provided_candidate(
    target: &BuildTarget,
    candidates: &dyn RevisionCandidateSource,
    guard: &BuildGuard,
    state: &StateStore,
    revisions: &RevisionStore,
) -> OpResult<DocumentationPublished, PublishDocumentationError> {
    let installation = target.installation();
    // Held across everything below, so the pointer read in step 2 is still the pointer step 5
    // publishes against. Released by `BuildHold`'s drop on every path, `?` included.
    let _hold = guard.hold(installation).map_err(|active| {
        Failure::Expected(PublishDocumentationError::BuildInProgress {
            installation: active,
        })
    })?;

    // One snapshot answers both questions, so they cannot disagree with each other.
    let snapshot = state.snapshot();
    if !snapshot
        .discovery_locations
        .iter()
        .any(|configured| configured.id == target.location())
    {
        return Err(Failure::Expected(
            PublishDocumentationError::InstallationUnavailable { installation },
        ));
    }
    let expected_prior = snapshot
        .publication_references
        .get(&installation)
        .map(|reference| reference.revision.clone());

    let candidate = candidates
        .candidate_for(installation)
        .map_err(|failure| match failure {
            Failure::Expected(unavailable) => {
                Failure::Expected(PublishDocumentationError::AnalysisFailed {
                    installation,
                    reason: match unavailable {
                        CandidateUnavailable::NoAnalysis => AnalysisFailure::NoAnalysisInThisBuild,
                        CandidateUnavailable::NothingDocumented => {
                            AnalysisFailure::NothingDocumentedForThisInstallation
                        }
                    },
                })
            }
            Failure::Unexpected(unexpected) => Failure::Unexpected(unexpected),
        })?;

    // The one thing `RevisionCandidate` cannot check for itself, and the reason the seam takes
    // an installation identifier rather than returning one: a candidate documenting somebody
    // else published under this target's location would store exactly the incoherent pair
    // `BuildTarget` exists to make unreachable.
    if candidate.installation != installation {
        return Err(Unexpected::new(format!(
            "candidate source returned a candidate documenting installation {} for a build \
             targeting {installation}",
            candidate.installation
        ))
        .into());
    }

    let published = revisions
        .publish(
            &candidate,
            target.location(),
            expected_prior.as_ref(),
            &PublicationCapability::new(state),
        )
        .map_err(|error| classify_publish(installation, error))?;

    Ok(DocumentationPublished {
        installation,
        revision: published.revision().into(),
        durability: durability_of(&published),
    })
}

/// Reads both commit points' durability off one [`Published`].
///
/// Separate and total, for the reason [`classify_publish`] is: every combination is reachable
/// from a test without arranging two independent filesystem conditions at once.
///
/// [`Published`]: crate::revisions::Published
fn durability_of(published: &Published) -> PublishedDurability {
    PublishedDurability {
        bundle_entry: match published.bundle_durability() {
            BundleDurability::Flushed => BundleEntryDurability::Flushed,
            BundleDurability::NotProvidedByPlatform => BundleEntryDurability::NotProvidedByPlatform,
        },
        publication_record: match published.pointer() {
            PointerCommit::Committed => PublicationRecordDurability::Flushed,
            PointerCommit::CommittedDurabilityUncertain => {
                PublicationRecordDurability::NotConfirmed
            }
        },
    }
}

/// Total over [`PublishError`], and separate from the function above so every arm is reachable
/// from a test without reproducing the filesystem state that produces it.
fn classify_publish(
    installation: ModInstallationId,
    error: PublishError,
) -> Failure<PublishDocumentationError> {
    let storage = |reason| {
        Failure::Expected(PublishDocumentationError::StorageUnavailable {
            installation,
            reason,
        })
    };
    match error {
        PublishError::StagingFailed(_) => storage(StorageFailure::RevisionNotStaged),
        PublishError::BundleNotCompleted { .. } => storage(StorageFailure::RevisionNotCompleted),
        PublishError::BundleDurabilityUnconfirmed { .. } => {
            storage(StorageFailure::RevisionDurabilityUnconfirmed)
        }
        PublishError::PublishedBundleUnusable { .. } => {
            storage(StorageFailure::RevisionsLocationUnusable)
        }
        // The design's own example of an unexpected failure: "failure to validate a bundle the
        // current build just generated". This one is about *our* staging tree, which nothing
        // outside the process can reach, so it means the writer and the validator disagree.
        PublishError::BundleInvalid { .. } => Unexpected::new(
            "a bundle this build staged failed its own validation before it was moved",
        )
        .into(),
        PublishError::PointerNotCommitted(pointer) => match pointer {
            // The late form of the up-front check: the location was removed between them.
            PublicationError::Mutation(MutationError::UnknownLocation) => Failure::Expected(
                PublishDocumentationError::InstallationUnavailable { installation },
            ),
            PublicationError::Mutation(MutationError::RecoveryRequired) => {
                storage(StorageFailure::StateRecoveryRequired)
            }
            PublicationError::Mutation(MutationError::StorageFailed { .. }) => {
                storage(StorageFailure::StateNotWritten)
            }
            // `set_publication_reference` writes no Discovery Location path, so nothing it does
            // can reach the encoder's one refusal.
            PublicationError::Mutation(MutationError::PathNotUtf8 { .. }) => Unexpected::new(
                "the publication reference commit reported a non-UTF-8 path, which it writes none of",
            )
            .into(),
            // Unreachable because `BuildGuard` spans the read of `expected_prior` and this
            // commit, and one Desktop Host owns one application-data directory (D-065). If it
            // happens, one of those two assumptions has stopped being true.
            PublicationError::ExpectedMismatch { .. } => Unexpected::new(
                "the published revision changed while a build held the publication guard",
            )
            .into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::version::AnalysisVersionVector;
    use crate::application::candidates::NoAnalysisSource;
    use crate::canonical::path::LogicalPath;
    use crate::discovery::identity::DiscoveryLocationId;
    use crate::revisions::{
        EntryList, EntrySummary, RevisionCandidate, RevisionDocument, RevisionInputs,
    };
    use crate::source::ObservationGaps;
    use crate::source::fingerprint::{ContentHash, SourceFingerprint};
    use crate::state::OpenOutcome;
    use crate::state::replace::{RealIo, ReplacementIo};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tempfile::TempDir;

    fn fingerprint(bytes: &[u8]) -> SourceFingerprint {
        SourceFingerprint::of(
            [(
                LogicalPath::parse("descriptor.mod").unwrap(),
                ContentHash::of(bytes),
            )],
            &ObservationGaps::default(),
        )
        .unwrap()
    }

    fn candidate(installation: ModInstallationId, marker: &str) -> RevisionCandidate {
        RevisionCandidate::new(
            installation,
            RevisionInputs {
                target_mod: fingerprint(b"name=\"Fixture\"\n"),
                vanilla_content: fingerprint(b"name=\"Stellaris\"\n"),
            },
            AnalysisVersionVector::current(),
            true,
            vec![RevisionDocument::EntryList(EntryList {
                entries: vec![EntrySummary {
                    category: "technology".to_owned(),
                    identifier: marker.to_owned(),
                    display_name: None,
                }],
            })],
        )
        .unwrap()
    }

    /// A source that answers with whatever it was built with, so a test names the outcome it
    /// wants rather than arranging for one.
    struct Answers(
        Box<
            dyn Fn(ModInstallationId) -> OpResult<RevisionCandidate, CandidateUnavailable>
                + Send
                + Sync,
        >,
    );

    impl RevisionCandidateSource for Answers {
        fn candidate_for(
            &self,
            installation: ModInstallationId,
        ) -> OpResult<RevisionCandidate, CandidateUnavailable> {
            (self.0)(installation)
        }
    }

    fn answering(marker: &'static str) -> Answers {
        Answers(Box::new(move |installation| {
            Ok(candidate(installation, marker))
        }))
    }

    /// An opened state store, an opened revisions store, and a target coherent with both.
    struct Host {
        _dir: TempDir,
        root: PathBuf,
        state: StateStore,
        revisions: RevisionStore,
        guard: BuildGuard,
        target: BuildTarget,
        /// Flipped to make every further state replacement fail. Held here rather than
        /// passed in because opening the store and configuring its Discovery Location both
        /// write, and a seam that refused from the start would fail the setup instead of the
        /// step under test.
        refuse_state_writes: Arc<AtomicBool>,
        /// Flipped to make the flush *after* a state replacement fail, which is how `state`
        /// reaches `CommittedDurabilityUncertain`: the new document is visible and its
        /// durability is unknown.
        refuse_state_flush: Arc<AtomicBool>,
    }

    impl Host {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let refuse = Arc::new(AtomicBool::new(false));
            let refuse_flush = Arc::new(AtomicBool::new(false));
            let OpenOutcome::Ready { store, .. } = StateStore::open_with_io(
                dir.path(),
                Box::new(RefusingWhenAsked {
                    inner: RealIo,
                    refuse: Arc::clone(&refuse),
                    refuse_flush: Arc::clone(&refuse_flush),
                }),
            )
            .unwrap() else {
                panic!("state store blocked");
            };
            let (location, _) = store
                .add_discovery_location(PathBuf::from("/tmp/workshop"))
                .unwrap();
            let root = dir.path().join("revisions");
            let revisions = RevisionStore::open(&root).unwrap();
            Self {
                _dir: dir,
                root,
                state: store,
                revisions,
                guard: BuildGuard::new(),
                target: BuildTarget::derive(location, LogicalPath::parse("ugc_1").unwrap()),
                refuse_state_writes: refuse,
                refuse_state_flush: refuse_flush,
            }
        }

        /// Makes every further state replacement fail, so a publication reaches its complete
        /// bundle and cannot commit the pointer.
        fn refuse_state_writes(&self) {
            self.refuse_state_writes.store(true, Ordering::SeqCst);
        }

        /// Makes the flush after every further state replacement fail, so the pointer commit
        /// succeeds with its durability unconfirmed.
        fn refuse_state_flush(&self) {
            self.refuse_state_flush.store(true, Ordering::SeqCst);
        }

        /// A target under a Discovery Location that was never configured.
        fn unconfigured_target(&self) -> BuildTarget {
            BuildTarget::derive(
                DiscoveryLocationId::generate(),
                LogicalPath::parse("ugc_2").unwrap(),
            )
        }

        fn publish(
            &self,
            target: &BuildTarget,
            candidates: &dyn RevisionCandidateSource,
        ) -> OpResult<DocumentationPublished, PublishDocumentationError> {
            publish_provided_candidate(
                target,
                candidates,
                &self.guard,
                &self.state,
                &self.revisions,
            )
        }

        fn published(&self, target: &BuildTarget) -> Option<RevisionId> {
            self.state
                .snapshot()
                .publication_references
                .get(&target.installation())
                .map(|reference| reference.revision.clone())
        }

        fn bundles(&self) -> Vec<PathBuf> {
            entries(&self.root.join("bundles"))
        }
    }

    fn entries(dir: &Path) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .collect();
        found.sort();
        found
    }

    /// Real replacement I/O until asked to stop, then refusal before the commit point — so
    /// the prior state is retained and the mutation reports `StorageFailed`.
    struct RefusingWhenAsked {
        inner: RealIo,
        refuse: Arc<AtomicBool>,
        refuse_flush: Arc<AtomicBool>,
    }

    impl ReplacementIo for RefusingWhenAsked {
        fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
            if self.refuse.load(Ordering::SeqCst) {
                return Err(std::io::Error::other("state volume is read-only"));
            }
            self.inner.write_temp(dir, bytes)
        }

        fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
            self.inner.rename(from, to)
        }

        fn sync_dir(&mut self, dir: &Path) -> std::io::Result<crate::durability::DirectoryFlush> {
            if self.refuse_flush.load(Ordering::SeqCst) {
                return Err(std::io::Error::other("directory flush refused"));
            }
            self.inner.sync_dir(dir)
        }
    }

    #[test]
    fn a_provided_candidate_becomes_the_installations_published_revision() {
        let host = Host::new();

        let published = host.publish(&host.target, &answering("tech_a")).unwrap();

        assert_eq!(published.installation, host.target.installation());
        assert_eq!(host.published(&host.target), Some(published.revision));
    }

    #[test]
    fn a_second_publication_replaces_the_first_it_named_as_its_prior() {
        // The `expected_prior` this coordinator reads is the whole reason a second publication
        // is a replacement rather than a refusal; nothing in `revisions` reads the pointer.
        let host = Host::new();
        let first = host.publish(&host.target, &answering("tech_a")).unwrap();

        let second = host.publish(&host.target, &answering("tech_b")).unwrap();

        assert_ne!(first.revision, second.revision);
        assert_eq!(host.published(&host.target), Some(second.revision));
    }

    #[test]
    fn publishing_for_an_unconfigured_location_refuses_before_any_bundle_is_written() {
        // The assertion the up-front check exists for. Without it this refusal still happens,
        // but only after a complete bundle has been moved onto its final path and flushed.
        let host = Host::new();
        let target = host.unconfigured_target();

        let refusal = host.publish(&target, &answering("tech_a")).unwrap_err();

        assert!(matches!(
            refusal,
            Failure::Expected(PublishDocumentationError::InstallationUnavailable { .. })
        ));
        assert!(host.bundles().is_empty());
    }

    #[test]
    fn a_source_that_produces_no_candidate_refuses_with_analysis_failed_and_publishes_nothing() {
        let host = Host::new();

        let refusal = host.publish(&host.target, &NoAnalysisSource).unwrap_err();

        assert!(matches!(
            refusal,
            Failure::Expected(PublishDocumentationError::AnalysisFailed {
                reason: AnalysisFailure::NoAnalysisInThisBuild,
                ..
            })
        ));
        assert_eq!(host.published(&host.target), None);
        assert!(host.bundles().is_empty());
    }

    #[test]
    fn the_candidate_source_is_the_reason_a_production_build_cannot_publish() {
        // The negative control for the test above. Same stores, same target, same call — only
        // the source differs. Without it, "a production build refuses" would also be satisfied
        // by a coordinator that never worked at all.
        let host = Host::new();

        assert!(host.publish(&host.target, &NoAnalysisSource).is_err());
        assert!(host.publish(&host.target, &answering("tech_a")).is_ok());
        assert!(host.published(&host.target).is_some());
    }

    #[test]
    fn a_state_document_that_cannot_be_written_refuses_with_storage_unavailable() {
        let host = Host::new();
        host.refuse_state_writes();

        let refusal = host
            .publish(&host.target, &answering("tech_a"))
            .unwrap_err();

        assert!(matches!(
            refusal,
            Failure::Expected(PublishDocumentationError::StorageUnavailable {
                reason: StorageFailure::StateNotWritten,
                ..
            })
        ));
        assert_eq!(host.published(&host.target), None);
    }

    #[test]
    fn a_publication_whose_record_flush_is_refused_reports_it_rather_than_claiming_durability() {
        // Reported by review, and reachable: reading `bundle_durability()` alone called this
        // publication durable while `state` had told us the opposite about the second commit
        // point. The bundle's own flush succeeds here — only the record's does not — so the
        // fact under test cannot be supplied by the other half.
        let host = Host::new();
        host.refuse_state_flush();

        let published = host.publish(&host.target, &answering("tech_a")).unwrap();

        assert_eq!(
            published.durability.bundle_entry,
            BundleEntryDurability::Flushed
        );
        assert_eq!(
            published.durability.publication_record,
            PublicationRecordDurability::NotConfirmed
        );
        assert!(!published.durability.is_fully_confirmed());
        // Still a publication: the revision is what a reader sees now.
        assert_eq!(host.published(&host.target), Some(published.revision));
    }

    #[test]
    fn an_ordinary_publication_confirms_both_commit_points() {
        // The negative control for the test above: without it, "not fully confirmed" could be
        // what this coordinator always reports.
        let host = Host::new();

        let published = host.publish(&host.target, &answering("tech_a")).unwrap();

        assert!(published.durability.is_fully_confirmed());
    }

    #[test]
    fn a_candidate_naming_another_installation_is_a_defect_rather_than_a_refusal() {
        let host = Host::new();
        let impostor = ModInstallationId::parse(&"cd".repeat(32)).unwrap();
        let source = Answers(Box::new(move |_| Ok(candidate(impostor, "tech_a"))));

        let failure = host.publish(&host.target, &source).unwrap_err();

        assert!(matches!(failure, Failure::Unexpected(_)));
        assert_eq!(host.published(&host.target), None);
    }

    #[test]
    fn a_second_publication_is_refused_while_the_first_holds_the_guard() {
        // Taking the hold directly is what a concurrent request looks like from inside this
        // one: the coordinator refuses up front rather than queueing behind it, and the
        // refusal names what is running. A second thread would prove the same thing while
        // depending on a schedule.
        let host = Host::new();
        let held = host.guard.hold(host.target.installation()).unwrap();

        let refusal = host
            .publish(&host.target, &answering("tech_b"))
            .unwrap_err();
        assert!(matches!(
            refusal,
            Failure::Expected(PublishDocumentationError::BuildInProgress { installation })
                if installation == host.target.installation()
        ));
        drop(held);
        assert!(host.publish(&host.target, &answering("tech_b")).is_ok());
    }

    #[test]
    fn the_guard_names_the_installation_that_is_already_building() {
        let guard = BuildGuard::new();
        let first = ModInstallationId::parse(&"ab".repeat(32)).unwrap();
        let second = ModInstallationId::parse(&"ef".repeat(32)).unwrap();

        let Ok(held) = guard.hold(first) else {
            panic!("the first hold is granted");
        };
        assert_eq!(guard.hold(second).err(), Some(first));
        drop(held);
        assert!(guard.hold(second).is_ok());
    }

    #[test]
    fn every_publication_failure_classifies_without_naming_a_path() {
        // `classify_publish` is total over `PublishError`, and this is where each arm is
        // reached without reproducing the filesystem state that produces it. The path check is
        // the payload rule: a refusal carries an installation and a reason, never a directory.
        let installation = ModInstallationId::parse(&"ab".repeat(32)).unwrap();
        let rows = [
            (
                PublishError::BundleNotCompleted {
                    detail: "/Users/x/Library/secret".to_owned(),
                },
                Some(StorageFailure::RevisionNotCompleted),
            ),
            (
                PublishError::BundleDurabilityUnconfirmed {
                    detail: "/Users/x/Library/secret".to_owned(),
                },
                Some(StorageFailure::RevisionDurabilityUnconfirmed),
            ),
            (
                PublishError::PointerNotCommitted(PublicationError::Mutation(
                    MutationError::UnknownLocation,
                )),
                None,
            ),
            (
                PublishError::PointerNotCommitted(PublicationError::Mutation(
                    MutationError::RecoveryRequired,
                )),
                Some(StorageFailure::StateRecoveryRequired),
            ),
            (
                PublishError::PointerNotCommitted(PublicationError::ExpectedMismatch {
                    actual: None,
                }),
                None,
            ),
        ];

        for (error, expected) in rows {
            let classified = classify_publish(installation, error);
            match (&classified, expected) {
                (
                    Failure::Expected(PublishDocumentationError::StorageUnavailable {
                        reason, ..
                    }),
                    Some(wanted),
                ) => assert_eq!(*reason, wanted),
                (
                    Failure::Expected(PublishDocumentationError::InstallationUnavailable {
                        ..
                    }),
                    None,
                )
                | (Failure::Unexpected(_), None) => {}
                other => panic!("unexpected classification: {other:?}"),
            }
            if let Failure::Expected(expected) = &classified {
                assert!(!expected.to_string().contains("secret"));
                assert!(!format!("{expected:?}").contains("secret"));
            }
        }
    }

    #[test]
    fn publish_documentation_error_displays_and_implements_std_error() {
        let installation = ModInstallationId::parse(&"ab".repeat(32)).unwrap();
        let error = PublishDocumentationError::StorageUnavailable {
            installation,
            reason: StorageFailure::RevisionNotStaged,
        };
        assert!(error.to_string().contains(&installation.to_string()));
        let _: &dyn std::error::Error = &error;
        let _: &dyn std::error::Error = &AnalysisFailure::NoAnalysisInThisBuild;
        let _: &dyn std::error::Error = &StorageFailure::RevisionNotStaged;
    }
}
