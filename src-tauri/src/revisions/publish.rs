//! The filesystem steps of one revision publication, as a seam, plus the production
//! implementation (docs/technical-design.md, "Documentation revision publication").
//!
//! Publication has two normative commit points — the durable move of a validated staging
//! directory onto its final path, and the state publication-reference replacement — and
//! the acceptance criterion is that a crash before or after either one leaves readers on
//! the prior complete revision or on the replacement, never on staging state. That claim
//! cannot be established on a healthy filesystem by argument, so every step a crash can
//! land between is named here and can be failed individually by a test double.
//!
//! **Reads are deliberately not in this seam**, for the reason `state::replace`
//! (`src/state/replace.rs:20`) records for its own: a read is not a commit point, and a
//! corruption test is more honest when it corrupts real bytes on a real disk than when it
//! asks a double to lie about what a file contains. [`stage::validate`] therefore reads
//! with plain `std::fs`, and the crash matrix injects failures only where an interrupted
//! write can actually leave the tree in an intermediate shape.
//!
//! [`stage::validate`]: crate::revisions::stage::validate

use crate::discovery::identity::DiscoveryLocationId;
use crate::revisions::candidate::RevisionCandidate;
use crate::revisions::manifest::RevisionIdentity;
use crate::revisions::stage::{
    BundleRefusal, StageError, bundle_path, bundles_root, stage_bundle, validate_as,
};
use crate::state::{MutationCommit, PublicationCapability, PublicationError, RevisionId};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// The externally observable I/O steps of one publication, in protocol order.
/// Production is [`RealPublicationIo`].
pub trait PublicationIo {
    /// Create a directory and every missing parent. Idempotent, and deliberately so: the
    /// per-attempt staging name is a fresh UUID, so "already exists" cannot mean another
    /// attempt's directory, while `staging/` and `bundles/` are shared and long-lived.
    fn create_dir(&mut self, path: &Path) -> io::Result<()>;
    /// Create, write, and durably flush one bundle file, creating parents.
    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    /// Durably flush a directory's entries where the platform provides it.
    fn sync_dir(&mut self, path: &Path) -> io::Result<()>;
    /// Atomically move a validated staging directory onto its final path. Commit point 1.
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()>;
    /// Best-effort removal of an abandoned staging directory.
    fn remove_dir_all(&mut self, path: &Path) -> io::Result<()>;
}

pub struct RealPublicationIo;

impl PublicationIo for RealPublicationIo {
    fn create_dir(&mut self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    /// No partial file is removed when a write fails, and that is the difference from
    /// [`ReplacementIo::write_temp`](crate::state::replace::ReplacementIo::write_temp):
    /// there the temporary file is the unit of abandonment, here the whole staging
    /// directory is. A half-written entry inside a staging directory that is never
    /// renamed is removed with that directory by the retention sweep, which recognizes it
    /// by name; removing the file alone would leave the directory behind anyway.
    ///
    /// **Per-file `sync_all` will not scale, and this is where that is fixed.** Phase 6
    /// stages 712–1,475 files per revision, and one flush each is the wrong shape at that
    /// count; "write everything, then flush the tree once" is the intended replacement.
    /// It is deliberately not done yet: the durability policy lives entirely behind this
    /// method, so the change is one implementation with the crash matrix already standing
    /// to re-prove it, and correctness before measurement costs nothing at Phase 3's one
    /// document.
    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(path)?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    fn sync_dir(&mut self, path: &Path) -> io::Result<()> {
        fs::File::open(path)?.sync_all()
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        // Directory rename, not file replacement: POSIX `rename` refuses a non-empty
        // destination directory with ENOTEMPTY rather than replacing it, so this is not
        // the "replace whatever is there" operation `state::replace` performs. The
        // protocol (Task 6) therefore never renames onto an occupied final path; an
        // already-present bundle is adopted after validation instead.
        fs::rename(from, to)
    }

    fn remove_dir_all(&mut self, path: &Path) -> io::Result<()> {
        fs::remove_dir_all(path)
    }
}

/// One completed publication: the revision that was published, and how far each of the two
/// commit points got.
///
/// **It carries no filesystem path, deliberately.** STE-17's Revision Reader owns bundle
/// addressing, and handing a path back to the application would invite it to open bundle
/// files directly — exactly what "`revisions` is the sole owner of Documentation Revision
/// bundle I/O" forbids (docs/technical-design.md, "Revision reader"). The identifier is
/// enough to ask the reader for the bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Published {
    revision: RevisionIdentity,
    bundle: BundleCompletion,
    pointer: PointerCommit,
}

impl Published {
    pub fn revision(&self) -> RevisionIdentity {
        self.revision
    }

    pub fn bundle(&self) -> BundleCompletion {
        self.bundle
    }

    pub fn pointer(&self) -> PointerCommit {
        self.pointer
    }
}

/// How commit point 1 — bundle completion — was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleCompletion {
    /// This publication moved its validated staging directory onto the final path.
    Created,
    /// The final path already held this exact bundle, validated before it was adopted.
    ///
    /// The expected shape after a crash between the two commit points: an identical
    /// rebuild derives the identifier the earlier attempt derived, so it finds its own
    /// work already complete and finishes the publication instead of failing forever.
    AlreadyComplete,
}

/// How commit point 2 — publication — was reached. Both variants mean the pointer names
/// the new revision; they differ only in what is known about durability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerCommit {
    Committed,
    /// The pointer is visible but its durability was not confirmed. Carried as a success
    /// rather than an error because `state` already reopened and validated the
    /// authoritative path: the new revision *is* what a reader sees.
    ///
    /// It carries exactly one consequence — **the superseded bundle is not yet
    /// retirable**. "The previous bundle becomes eligible for retirement only after the
    /// replacement pointer is observed as committed" (docs/technical-design.md, "Revision
    /// bundles"), so a retention sweep must be able to tell this apart from
    /// [`PointerCommit::Committed`], which is why it is a variant and not a flattened
    /// success.
    CommittedDurabilityUncertain,
}

/// Every way one publication can stop, each naming what is true on disk afterwards.
///
/// No variant carries a bundle path, for the reason [`Published`] carries none.
#[derive(Debug)]
pub enum PublishError {
    /// Staging did not finish. Nothing was moved, the previously published revision is
    /// untouched, and the partial staging directory was removed best-effort.
    StagingFailed(StageError),
    /// What was staged is not this revision's bundle: it disagreed with its own manifest,
    /// or that manifest is not the one staging sealed. Nothing was moved, and the staging
    /// directory was removed best-effort — it is damaged evidence of a build this process
    /// can simply repeat, and the refusal is the diagnostic.
    BundleInvalid { refusal: BundleRefusal },
    /// The final path is occupied by something that is not this revision's bundle. Nothing
    /// was moved, the pointer is unchanged, and **the staging directory is retained**: it
    /// is the only evidence of the intended replacement. The occupant is never deleted — a
    /// pinned reader may hold it, whether it is a damaged copy of this revision or an
    /// intact copy of another one, and repair is a retention concern for a later phase
    /// (docs/technical-design.md, "Revision bundles").
    PublishedBundleUnusable { refusal: BundleRefusal },
    /// The move reported failure and the final path does not hold a complete bundle, so
    /// commit point 1 did not pass. Nothing is published, the pointer is unchanged, and
    /// the staging directory was removed best-effort.
    BundleNotCompleted { detail: String },
    /// The bundle is at its final path but the directory entry naming it was not durably
    /// flushed, so the pointer commit was refused. Nothing is published and the pointer is
    /// unchanged; an identical rebuild takes the [`BundleCompletion::AlreadyComplete`]
    /// path and flushes it again.
    BundleDurabilityUnconfirmed { detail: String },
    /// Commit point 1 passed and commit point 2 did not: a complete, unreferenced bundle
    /// is at its final path while the previous revision stays published. That is the
    /// design's stated outcome for a crash between the commit points
    /// (docs/technical-design.md, "Revision bundles"), not a corruption.
    ///
    /// The `state` outcome is carried through [`std::error::Error::source`] rather than
    /// re-discriminated into variants here: `state` owns that outcome space, and a second
    /// listing of it would be a second authority on what a refused pointer commit means.
    PointerNotCommitted(PublicationError),
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StagingFailed(error) => write!(f, "no revision was published: {error}"),
            Self::BundleInvalid { refusal } => {
                write!(f, "the staged revision was not moved: {refusal}")
            }
            Self::PublishedBundleUnusable { refusal } => write!(
                f,
                "another revision directory already occupies this revision's path and was \
                 left untouched: {refusal}"
            ),
            Self::BundleNotCompleted { detail } => {
                write!(f, "no revision was published: {detail}")
            }
            Self::BundleDurabilityUnconfirmed { detail } => write!(
                f,
                "the revision was written but its durability could not be confirmed, so it \
                 was not published: {detail}"
            ),
            Self::PointerNotCommitted(error) => write!(
                f,
                "the revision was written but the previously published revision is still in \
                 use: {error}"
            ),
        }
    }
}

impl std::error::Error for PublishError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::StagingFailed(error) => Some(error),
            Self::BundleInvalid { refusal } | Self::PublishedBundleUnusable { refusal } => {
                Some(refusal)
            }
            Self::BundleNotCompleted { .. } | Self::BundleDurabilityUnconfirmed { .. } => None,
            Self::PointerNotCommitted(error) => Some(error),
        }
    }
}

/// Publishes one Revision Candidate: stage, validate, move, flush, and commit the pointer.
///
/// The ten steps run in this order, and the two ★ steps are the commit points
/// (docs/technical-design.md, "Revision bundles"):
///
/// ```text
/// 0  accept the candidate                        — discharged by the type, see below
/// 1  create the staging directory                \
/// 2  write each document, recording its hash      |  stage_bundle, which also owns
/// 3  seal the manifest, deriving the identifier   |  the staged subtree's fsyncs
/// 4  write the manifest last                      |
/// 5  fsync the staged tree, deepest first        /
/// 6  validate the staged bundle from disk
/// 7★ rename staging -> bundles/<hex>             — bundle completion
/// 8  fsync bundles/
/// 9★ replace the publication reference           — publication
/// ```
///
/// **Step 0 cannot fail here.** [`RevisionCandidate::new`] is the parse, and holding one is
/// the evidence that it succeeded; a runtime check at this boundary would be a second,
/// weaker authority over what a publishable candidate is.
///
/// The fsyncs are deliberately split across two owners and named in one place so the
/// sequence reads whole: [`stage_bundle`] flushes everything inside the staging directory
/// (step 5), and step 8 flushes the `bundles/` directory entry the move created. Neither
/// half is sufficient alone — file contents that have not reached disk make the bundle
/// incomplete, and a directory entry that has not reached disk makes the whole bundle
/// disappear.
pub(super) fn publish_revision(
    io_seam: &mut dyn PublicationIo,
    revisions_root: &Path,
    candidate: &RevisionCandidate,
    location: DiscoveryLocationId,
    expected_prior: Option<&RevisionId>,
    publication: &PublicationCapability<'_>,
) -> Result<Published, PublishError> {
    // Steps 1-5.
    let staged = match stage_bundle(io_seam, revisions_root, candidate) {
        Ok(staged) => staged,
        Err(error) => {
            discard_staging(io_seam, error.staging());
            return Err(PublishError::StagingFailed(error));
        }
    };
    let revision = staged.identity();
    let final_path = bundle_path(revisions_root, revision);

    // Step 6. `ValidatedBundle` exists so this cannot be skipped: the move below consumes
    // what only `validate` constructs. `validate_as` and not `validate`, because
    // `final_path` was computed from the identity `stage_bundle` reported in memory while
    // validation reads the manifest back from disk: if the two ever disagreed, this
    // publication would move a bundle for one revision onto another revision's path. That
    // is the same claim the adoption branch below guards, made where the name is chosen.
    let validated = match validate_as(staged.root(), revision) {
        Ok(validated) => validated,
        Err(refusal) => {
            discard_staging(io_seam, staged.root());
            return Err(PublishError::BundleInvalid { refusal });
        }
    };

    // Step 7 ★. An occupied final path is never renamed onto: a directory rename refuses a
    // non-empty destination (ENOTEMPTY) rather than replacing it, so what the destination
    // already holds decides the outcome before the move is attempted.
    let bundle = match observe_final_path(&final_path, revision) {
        FinalPath::Complete => {
            // The same bundle is already there — an identical rebuild after a crash
            // between the commit points. Adoption is sound because equal identifiers mean
            // equal manifest bodies, validation's `unexpected` check proves the directory
            // holds nothing besides what that manifest names, and `validate_as` proves
            // the manifest is this revision's rather than merely some revision's.
            discard_staging(io_seam, staged.root());
            BundleCompletion::AlreadyComplete
        }
        FinalPath::Occupied(refusal) => {
            // Staging is deliberately kept: it is the only evidence of the intended
            // replacement, and the occupant is never deleted.
            return Err(PublishError::PublishedBundleUnusable { refusal });
        }
        FinalPath::Absent => match io_seam.rename(validated.path(), &final_path) {
            Ok(()) => BundleCompletion::Created,
            Err(error) => classify_reported_rename_failure(
                io_seam,
                staged.root(),
                &final_path,
                revision,
                &format!("moving the revision to its final path failed: {error}"),
            )?,
        },
    };

    // Step 8. Also run on the adoption path: an earlier attempt may have reached exactly
    // here and failed, and this is the rebuild that is supposed to repair that.
    if let Err(error) = io_seam.sync_dir(&bundles_root(revisions_root)) {
        // Deliberately asymmetric with `state`, which floors at
        // `CommittedDurabilityUncertain` and advances memory. There the new state *is*
        // what is visible, and claiming otherwise would be a lie. Here the pointer commit
        // would be a *further irreversible action on an unconfirmed foundation*: a pointer
        // naming a bundle a crash can still erase makes reads fail closed, and it breaks
        // retention's assumption that a pointer names a complete bundle. Refusing costs
        // one rebuild, which then takes the `AlreadyComplete` path above.
        return Err(PublishError::BundleDurabilityUnconfirmed {
            detail: format!("flushing the revisions directory failed: {error}"),
        });
    }

    // Step 9 ★. The one and only state mutation this module performs, and the only one it
    // can express: `expected_prior` comes from the caller, because the caller is the only
    // place that knows which revision it intends to replace. `revisions` never reads the
    // pointer it is replacing.
    let pointer = match publication.set_publication_reference(
        candidate.installation,
        location,
        expected_prior,
        revision.into(),
    ) {
        Ok(MutationCommit::Committed) => PointerCommit::Committed,
        // Not reconciled here. `state` already reopened and validated the authoritative
        // path; a second attempt would be a second authority on which revision is
        // published.
        Ok(MutationCommit::CommittedDurabilityUncertain) => {
            PointerCommit::CommittedDurabilityUncertain
        }
        Err(error) => return Err(PublishError::PointerNotCommitted(error)),
    };

    Ok(Published {
        revision,
        bundle,
        pointer,
    })
}

/// What the final bundle path holds. The one place that question is answered, so the
/// pre-move check and the reported-failure reclassification below cannot drift apart.
enum FinalPath {
    Absent,
    /// A complete bundle of `expected` — the only occupant that may be adopted.
    Complete,
    /// Present and not that. A valid bundle of *another* revision lands here rather than
    /// in `Complete`: the directory is undamaged, but it says nothing about the revision
    /// this publication is trying to complete, so it cannot be adopted as one.
    Occupied(BundleRefusal),
}

fn observe_final_path(path: &Path, expected: RevisionIdentity) -> FinalPath {
    // Read with plain `std::fs` rather than through the seam, for the reason
    // [`PublicationIo`] records: a read is not a commit point.
    if !path.exists() {
        return FinalPath::Absent;
    }
    match validate_as(path, expected) {
        Ok(_) => FinalPath::Complete,
        Err(refusal) => FinalPath::Occupied(refusal),
    }
}

/// The only ambiguous step: a move that reported failure may have completed before the
/// error surfaced. The destination decides, never the error — exactly as
/// `state::replace::classify_reported_rename_failure` decides for state
/// (`src/state/replace.rs:161-183`).
///
/// The staging directory is the second observation, and it is what tells the two
/// indistinguishable "the destination holds our bundle" causes apart: a rename that moved
/// the tree leaves no source behind, so staging still being present means the destination
/// was already complete before this attempt.
fn classify_reported_rename_failure(
    io_seam: &mut dyn PublicationIo,
    staging: &Path,
    final_path: &Path,
    expected: RevisionIdentity,
    detail: &str,
) -> Result<BundleCompletion, PublishError> {
    match observe_final_path(final_path, expected) {
        FinalPath::Complete if staging.exists() => {
            discard_staging(io_seam, staging);
            Ok(BundleCompletion::AlreadyComplete)
        }
        FinalPath::Complete => Ok(BundleCompletion::Created),
        FinalPath::Occupied(refusal) => Err(PublishError::PublishedBundleUnusable { refusal }),
        FinalPath::Absent => {
            discard_staging(io_seam, staging);
            Err(PublishError::BundleNotCompleted {
                detail: detail.to_owned(),
            })
        }
    }
}

/// Removes a staging directory that will never be renamed, best-effort.
///
/// The same rule and the same reason as `RealIo::write_temp_in` removing its partial file
/// (`src/state/replace.rs:55-82`): the attempt's name is a fresh UUID no later attempt can
/// choose, so an abandoned directory is unreachable, and leaving one behind accumulates
/// junk in the user's application-data directory on every ENOSPC. Best-effort because the
/// retention sweep recognizes the same directories by name — that is a reason not to fail
/// a publication over a failed cleanup, not a reason to skip the cleanup.
fn discard_staging(io_seam: &mut dyn PublicationIo, staging: &Path) {
    let _ = io_seam.remove_dir_all(staging);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::version::AnalysisVersionVector;
    use crate::canonical::path::LogicalPath;
    use crate::discovery::identity::{DiscoveryLocationId, ModInstallationId};
    use crate::revisions::RevisionStore;
    use crate::revisions::candidate::{
        EntryList, EntrySummary, RevisionCandidate, RevisionDocument, RevisionInputs,
    };
    use crate::revisions::stage::{
        RevisionMismatch, ValidationReport, bundle_path, bundles_root, stage_bundle, staging_root,
        validate, validate_as,
    };
    use crate::source::ObservationGaps;
    use crate::source::fingerprint::{ContentHash, SourceFingerprint};
    use crate::state::replace::{RealIo, ReplacementIo};
    use crate::state::{
        MutationError, OpenOutcome, PublicationCapability, PublicationError, RevisionId, StateStore,
    };
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    /// The manifest's name inside a bundle, as the seam sees it going past. `stage` owns
    /// the layout and keeps its constant private; a test that must recognize one write out
    /// of several has no other handle on it.
    const MANIFEST_NAME: &str = "manifest.json";

    /// The state store, its Discovery Location, and the installation a candidate documents
    /// there. Assembled together because `set_publication_reference` refuses an unknown
    /// location, so every publication test needs the pair to be coherent.
    struct Pointer {
        store: StateStore,
        location: DiscoveryLocationId,
        installation: ModInstallationId,
    }

    impl Pointer {
        fn opened(dir: &Path, io_seam: Box<dyn ReplacementIo + Send>) -> Self {
            fs::create_dir_all(dir).unwrap();
            let outcome = StateStore::open_with_io(dir, io_seam).unwrap();
            let OpenOutcome::Ready { store, .. } = outcome else {
                panic!("state store blocked");
            };
            let (location, installation) = install(&store, "/tmp/workshop", "ugc_1");
            Self {
                store,
                location,
                installation,
            }
        }

        fn capability(&self) -> PublicationCapability<'_> {
            PublicationCapability::new(&self.store)
        }

        /// The revision the state document currently names for this installation.
        fn published(&self) -> Option<RevisionId> {
            self.store
                .snapshot()
                .publication_references
                .get(&self.installation)
                .map(|reference| reference.revision.clone())
        }
    }

    /// Adds a Discovery Location and derives one installation under it, which is the only
    /// pair `set_publication_reference` accepts.
    fn install(
        store: &StateStore,
        path: &str,
        relative: &str,
    ) -> (DiscoveryLocationId, ModInstallationId) {
        let (location, _) = store.add_discovery_location(PathBuf::from(path)).unwrap();
        let installation =
            ModInstallationId::derive(location, &LogicalPath::parse(relative).unwrap());
        (location, installation)
    }

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

    /// A candidate for `installation` whose one entry carries `marker`, so two candidates
    /// that differ only in `marker` are two different revisions.
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

    /// The identifier this candidate will always derive, obtained by staging it once in a
    /// throwaway root with real I/O.
    ///
    /// The identifier is a function of the candidate and the bytes its documents encode
    /// to, never of where or when it was staged — which is what lets a row whose
    /// publication never reaches step 3 still name the bundle path that must not exist.
    fn identity_of(candidate: &RevisionCandidate) -> RevisionIdentity {
        let scratch = TempDir::new().unwrap();
        stage_bundle(&mut RealPublicationIo, scratch.path(), candidate)
            .unwrap()
            .identity()
    }

    fn entries(dir: &Path) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .collect();
        found.sort();
        found
    }

    /// One named step of the publication protocol, so a row of the crash matrix can say
    /// exactly where the crash lands. [`FailAt`] fails every step it is given and performs
    /// every other one for real, which is what makes a row's outcome attributable to the
    /// step it names rather than to a store that never worked.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Step {
        /// Step 1, and only this attempt's directory: `bundles/` and `staging/` are created
        /// when the store is opened, and failing those would fail the open instead.
        CreateStagingDirectory,
        /// Step 2.
        WriteDocument,
        /// Step 4.
        WriteManifest,
        /// Step 5, the flush of the staged subtree.
        FlushStagedTree,
        /// Rewrites a document after `stage_bundle` hashed it, which is what a corrupted
        /// staged bundle looks like from the validator's side.
        CorruptAfterWriting,
        /// Step 7 ★.
        Move,
        /// Step 7 ★, ambiguous: the move happens and the seam reports failure anyway.
        MoveThenReportFailure,
        /// Step 7 ★, ambiguous the other way: the move does *not* happen, and the
        /// destination is completed by someone else before the error is classified.
        DestinationCompletedByAnotherWriter,
        /// Step 8.
        FlushRevisionsDirectory,
        /// The best-effort removal of an attempt that will never be renamed.
        DiscardStaging,
    }

    struct FailAt {
        steps: Vec<Step>,
        revisions_root: PathBuf,
        inner: RealPublicationIo,
    }

    impl FailAt {
        fn seam(steps: &[Step], revisions_root: &Path) -> Box<dyn PublicationIo + Send> {
            Box::new(Self {
                steps: steps.to_vec(),
                revisions_root: revisions_root.to_path_buf(),
                inner: RealPublicationIo,
            })
        }

        fn fails(&self, step: Step) -> bool {
            self.steps.contains(&step)
        }

        fn refuse(step: Step) -> io::Error {
            io::Error::other(format!("injected {step:?} failure"))
        }
    }

    impl PublicationIo for FailAt {
        fn create_dir(&mut self, path: &Path) -> io::Result<()> {
            if self.fails(Step::CreateStagingDirectory)
                && path.parent() == Some(staging_root(&self.revisions_root).as_path())
            {
                return Err(Self::refuse(Step::CreateStagingDirectory));
            }
            self.inner.create_dir(path)
        }

        fn write_file(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
            let manifest = path.file_name().is_some_and(|name| name == MANIFEST_NAME);
            if manifest && self.fails(Step::WriteManifest) {
                return Err(Self::refuse(Step::WriteManifest));
            }
            if !manifest && self.fails(Step::WriteDocument) {
                return Err(Self::refuse(Step::WriteDocument));
            }
            self.inner.write_file(path, bytes)
        }

        fn sync_dir(&mut self, path: &Path) -> io::Result<()> {
            if self.fails(Step::FlushRevisionsDirectory)
                && path == bundles_root(&self.revisions_root)
            {
                return Err(Self::refuse(Step::FlushRevisionsDirectory));
            }
            if self.fails(Step::FlushStagedTree)
                && path.starts_with(staging_root(&self.revisions_root))
            {
                return Err(Self::refuse(Step::FlushStagedTree));
            }
            if self.fails(Step::CorruptAfterWriting)
                && path.file_name().is_some_and(|name| name == "documents")
            {
                fs::write(path.join("entry-list.json"), b"tampered after the write").unwrap();
            }
            self.inner.sync_dir(path)
        }

        fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
            if self.fails(Step::Move) {
                return Err(Self::refuse(Step::Move));
            }
            if self.fails(Step::DestinationCompletedByAnotherWriter) {
                // What a second process's identical publication leaves behind if it wins
                // the race between the pre-move check and this rename: the destination is
                // complete and this attempt's own directory is untouched.
                copy_tree(from, to)?;
                return Err(Self::refuse(Step::DestinationCompletedByAnotherWriter));
            }
            self.inner.rename(from, to)?;
            if self.fails(Step::MoveThenReportFailure) {
                return Err(Self::refuse(Step::MoveThenReportFailure));
            }
            Ok(())
        }

        fn remove_dir_all(&mut self, path: &Path) -> io::Result<()> {
            if self.fails(Step::DiscardStaging) {
                return Err(Self::refuse(Step::DiscardStaging));
            }
            self.inner.remove_dir_all(path)
        }
    }

    /// Copies a staged tree, the only way a test can put a complete bundle at a
    /// destination without moving the attempt that is supposed to stay behind.
    fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let target = to.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_tree(&entry.path(), &target)?;
            } else {
                fs::copy(entry.path(), target)?;
            }
        }
        Ok(())
    }

    /// Fails the state-replacement steps a test switches on, so one store can be opened and
    /// seeded with real I/O and then driven into a failing pointer commit. The `state`
    /// idiom (src/state/mutations.rs:555-584), reused rather than reinvented.
    ///
    /// The pointer side is a **real [`StateStore`] behind a failing [`ReplacementIo`]**,
    /// never a fake state: [`PointerCommit`] and [`PublishError::PointerNotCommitted`] are
    /// claims about `state`'s replacement semantics, and a double that answered for `state`
    /// would let those claims be true of nothing.
    #[derive(Clone, Copy, Default)]
    struct StateScript {
        fail_rename: bool,
        fail_sync_dir: bool,
    }

    struct ScriptedState {
        script: Arc<Mutex<StateScript>>,
        inner: RealIo,
    }

    impl ReplacementIo for ScriptedState {
        fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
            self.inner.write_temp(dir, bytes)
        }
        fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
            if self.script.lock().unwrap().fail_rename {
                return Err(io::Error::other("injected state rename failure"));
            }
            self.inner.rename(from, to)
        }
        fn sync_dir(&mut self, dir: &Path) -> io::Result<()> {
            if self.script.lock().unwrap().fail_sync_dir {
                return Err(io::Error::other("injected state flush failure"));
            }
            self.inner.sync_dir(dir)
        }
    }

    /// What stands at one revision's canonical bundle path.
    #[derive(Debug, PartialEq, Eq)]
    enum BundleAtFinalPath {
        Absent,
        /// A complete bundle of exactly that revision: the only shape a reader may open.
        ThisRevision,
        /// Present and not that — damaged, incomplete, or a valid bundle of some other
        /// revision. One variant, because the consequence is one: it must not be adopted.
        NotThisRevision,
    }

    /// Three of the four observations every row of the crash matrix makes. The fourth is
    /// the outcome the protocol returned, asserted at the call site because its expected
    /// value is the one thing no shared helper can name.
    #[derive(Debug, PartialEq, Eq)]
    struct Observed {
        /// The publication reference in `state.snapshot()`: what a reader would resolve.
        reference: Option<RevisionId>,
        bundle: BundleAtFinalPath,
        /// Attempts still under `staging/`. Zero unless the row says why not.
        staging_attempts: usize,
    }

    /// Asserts the outcome is the staging failure *this* step caused. Five rows end in
    /// `StagingFailed`, so the variant alone would let a row pass on a failure it did not
    /// inject.
    fn staging_failed_at(error: &PublishError, step: Step) {
        let PublishError::StagingFailed(staging) = error else {
            panic!("expected a staging failure, got {error}");
        };
        let detail = staging.to_string();
        assert!(
            detail.contains(&format!("injected {step:?} failure")),
            "expected the {step:?} injection, got {detail}"
        );
    }

    fn bundle_at(root: &Path, revision: RevisionIdentity) -> BundleAtFinalPath {
        let path = bundle_path(root, revision);
        if !path.exists() {
            return BundleAtFinalPath::Absent;
        }
        match validate_as(&path, revision) {
            Ok(_) => BundleAtFinalPath::ThisRevision,
            Err(_) => BundleAtFinalPath::NotThisRevision,
        }
    }

    /// The strongest statement STE-16 can make of its central acceptance criterion — *a
    /// reader sees either the prior complete revision or the replacement, never staging
    /// state* — while no reader exists yet. Run at the end of every row.
    ///
    /// It asserts two things:
    ///
    /// - every directory under `bundles/` is a complete bundle of the revision its own
    ///   name names, so nothing half-written, half-moved, or misfiled is reachable there;
    /// - the publication reference, when set, names one of those directories.
    ///
    /// **What it does not prove.** It re-reads bytes and re-hashes them; it never decodes
    /// a document, so a bundle that is hash-valid and undecodable passes it (stage.rs,
    /// `a_bundle_whose_entry_list_is_undecodable_still_validates`). Nor does it observe a
    /// reader concurrent with a publication — it runs after each row, not during one.
    /// STE-17's typed Revision Reader is the upgrade: "the reader opened the revision the
    /// pointer names and served its documents" subsumes both claims, and should replace
    /// this helper rather than sit beside it.
    ///
    /// `damaged` names bundle directories the *test* planted or tampered with. The
    /// protocol never produces one, so every entry is a deliberate act of the row that
    /// passes it, and that row's comment says why.
    fn every_directory_under_bundles_always_validates(
        root: &Path,
        pointer: Option<&RevisionId>,
        damaged: &[&str],
    ) {
        for path in entries(&bundles_root(root)) {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if damaged.contains(&name.as_str()) {
                continue;
            }
            let identity = RevisionIdentity::parse(&name)
                .unwrap_or_else(|error| panic!("bundles/{name} is not a revision path: {error}"));
            if let Err(report) = validate_as(&path, identity) {
                panic!("bundles/{name} is reachable and did not validate: {report}");
            }
        }
        if let Some(reference) = pointer {
            assert!(
                bundles_root(root).join(&reference.0).is_dir(),
                "the publication reference names {}, which is not a directory under bundles/",
                reference.0
            );
        }
    }

    /// A revisions root and a state pointer under one temporary directory. Every row needs
    /// both halves, and the two must not share a directory: a state document beside
    /// `bundles/` would make "nothing else is under the revisions root" untestable.
    struct Fixture {
        _dir: TempDir,
        root: PathBuf,
        pointer: Pointer,
        state_script: Arc<Mutex<StateScript>>,
    }

    impl Fixture {
        fn new() -> Self {
            Self::opened(|_| {})
        }

        /// `seed` runs against the state directory before the store opens it — the only
        /// moment a test can plant a document the store has to react to.
        fn opened(seed: impl FnOnce(&Path)) -> Self {
            let dir = TempDir::new().unwrap();
            let state_dir = dir.path().join("state");
            fs::create_dir_all(&state_dir).unwrap();
            seed(&state_dir);
            let state_script = Arc::new(Mutex::new(StateScript::default()));
            let pointer = Pointer::opened(
                &state_dir,
                Box::new(ScriptedState {
                    script: Arc::clone(&state_script),
                    inner: RealIo,
                }),
            );
            Self {
                root: dir.path().join("revisions"),
                _dir: dir,
                pointer,
                state_script,
            }
        }

        fn candidate(&self, marker: &str) -> RevisionCandidate {
            candidate(self.pointer.installation, marker)
        }

        /// Publishes with real I/O and no expected prior.
        fn publish(&self, candidate: &RevisionCandidate) -> Result<Published, PublishError> {
            self.publish_expecting(candidate, None)
        }

        fn publish_expecting(
            &self,
            candidate: &RevisionCandidate,
            expected_prior: Option<&RevisionId>,
        ) -> Result<Published, PublishError> {
            RevisionStore::open(&self.root).unwrap().publish(
                candidate,
                self.pointer.location,
                expected_prior,
                &self.pointer.capability(),
            )
        }

        fn publish_failing_at(
            &self,
            steps: &[Step],
            candidate: &RevisionCandidate,
        ) -> Result<Published, PublishError> {
            RevisionStore::open_with_io(&self.root, FailAt::seam(steps, &self.root))
                .unwrap()
                .publish(
                    candidate,
                    self.pointer.location,
                    None,
                    &self.pointer.capability(),
                )
        }

        /// Plants a complete bundle for `content` at `revision`'s canonical path, by
        /// staging it for real and moving it there. The way a crashed earlier attempt, a
        /// restored backup, or a hand-copied directory arrives at a final path — none of
        /// which this protocol can distinguish from its own work by provenance.
        fn plant_bundle(&self, revision: RevisionIdentity, content: &RevisionCandidate) {
            fs::create_dir_all(bundles_root(&self.root)).unwrap();
            let staged = stage_bundle(&mut RealPublicationIo, &self.root, content).unwrap();
            fs::rename(staged.root(), bundle_path(&self.root, revision)).unwrap();
        }

        /// The three disk-and-state observations of one row, taken together with the
        /// invariant that stands in for a reader.
        fn observe(&self, revision: RevisionIdentity, damaged: &[&str]) -> Observed {
            let reference = self.pointer.published();
            every_directory_under_bundles_always_validates(&self.root, reference.as_ref(), damaged);
            Observed {
                reference,
                bundle: bundle_at(&self.root, revision),
                staging_attempts: entries(&staging_root(&self.root)).len(),
            }
        }
    }

    #[test]
    fn the_injection_seam_is_the_reason_every_failing_row_fails() {
        // The negative control for the whole matrix (AGENTS.md, Testing: "prove
        // sophisticated tests and gates can go red with a deliberate negative control"). A
        // row that names a step is evidence only if naming it is what changed the outcome.
        // Two publications of the same candidate through the same seam type, differing
        // only in the step named, must reach different outcomes — otherwise a green row
        // could mean the protocol never ran rather than that it behaved.
        let control = Fixture::new();
        let published = control
            .publish_failing_at(&[], &control.candidate("tech_a"))
            .unwrap();
        assert_eq!(published.bundle(), BundleCompletion::Created);
        assert_eq!(published.pointer(), PointerCommit::Committed);

        let injected = Fixture::new();
        let error = injected
            .publish_failing_at(&[Step::Move], &injected.candidate("tech_a"))
            .unwrap_err();
        assert!(
            matches!(error, PublishError::BundleNotCompleted { .. }),
            "{error}"
        );
        // And the injected error is the seam's, not some unrelated I/O failure that would
        // make the row green for the wrong reason.
        assert!(
            error.to_string().contains("injected Move failure"),
            "{error}"
        );
    }

    #[test]
    fn a_clean_publication_completes_the_bundle_and_commits_the_pointer() {
        // The chain this ticket exists to prove, and the control every failing row is read
        // against: a candidate becomes a validated bundle at its canonical path and the
        // state pointer names it (docs/technical-design.md, "Revision bundles"). Both
        // commit points passed, so the outcome names a new bundle and a committed pointer.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let published = fixture.publish(&candidate).unwrap();

        assert_eq!(published.bundle(), BundleCompletion::Created);
        assert_eq!(published.pointer(), PointerCommit::Committed);
        // The final path is the identifier's, and the identifier is the candidate's alone.
        assert_eq!(published.revision(), identity_of(&candidate));
        assert_eq!(
            fixture.observe(published.revision(), &[]),
            Observed {
                reference: Some(published.revision().into()),
                bundle: BundleAtFinalPath::ThisRevision,
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn a_failed_staging_directory_creation_publishes_nothing() {
        // The earliest step a crash can land on, and the one where the cleanup that
        // follows every staging failure has nothing to remove: `discard_staging` is
        // best-effort precisely so a directory that was never created cannot turn a clean
        // refusal into a different outcome.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let error = fixture
            .publish_failing_at(&[Step::CreateStagingDirectory], &candidate)
            .unwrap_err();

        staging_failed_at(&error, Step::CreateStagingDirectory);
        assert_eq!(
            fixture.observe(identity_of(&candidate), &[]),
            Observed {
                reference: None,
                bundle: BundleAtFinalPath::Absent,
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn a_failed_document_write_publishes_nothing_and_removes_the_partial_directory() {
        // A crash inside step 2, before any manifest exists. What it leaves is the shape
        // the write order was chosen to bound — documents that nothing claims — and it is
        // removed rather than accumulated, the rule `RealIo::write_temp_in` records
        // (src/state/replace.rs:55-82): an attempt named by a UUID no later attempt can
        // choose is unreachable, and one is left behind on every ENOSPC.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let error = fixture
            .publish_failing_at(&[Step::WriteDocument], &candidate)
            .unwrap_err();

        staging_failed_at(&error, Step::WriteDocument);
        assert_eq!(
            fixture.observe(identity_of(&candidate), &[]),
            Observed {
                reference: None,
                bundle: BundleAtFinalPath::Absent,
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn a_failed_manifest_write_publishes_nothing_and_removes_the_partial_directory() {
        // The manifest is written last, so this discards the most work any staging failure
        // can: every document is on disk and nothing names them. That directory is not a
        // bundle — with no manifest there is nothing to judge it against — so even if the
        // removal were skipped it could never be adopted at a final path.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let error = fixture
            .publish_failing_at(&[Step::WriteManifest], &candidate)
            .unwrap_err();

        staging_failed_at(&error, Step::WriteManifest);
        let PublishError::StagingFailed(staging) = &error else {
            unreachable!("staging_failed_at already matched")
        };
        assert!(!staging.staging().exists());
        assert_eq!(
            fixture.observe(identity_of(&candidate), &[]),
            Observed {
                reference: None,
                bundle: BundleAtFinalPath::Absent,
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn a_failed_flush_of_the_staged_tree_publishes_nothing() {
        // Step 5 is a durability step, not a formality. The move commits whatever the
        // staged subtree is, and a subtree whose directory entries have not reached disk is
        // a bundle a crash can still empty — a complete-looking directory the pointer would
        // then be free to name. Refusing costs one rebuild.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let error = fixture
            .publish_failing_at(&[Step::FlushStagedTree], &candidate)
            .unwrap_err();

        staging_failed_at(&error, Step::FlushStagedTree);
        assert_eq!(
            fixture.observe(identity_of(&candidate), &[]),
            Observed {
                reference: None,
                bundle: BundleAtFinalPath::Absent,
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn a_document_corrupted_after_it_was_hashed_is_never_moved() {
        // Step 6 stands between staging and the first commit point and reads the directory
        // back from disk rather than trusting what was just written: a document rewritten
        // after it was hashed is exactly the shape an in-memory check would miss. Nothing
        // reaches `bundles/`, so no reader can ever observe it.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let error = fixture
            .publish_failing_at(&[Step::CorruptAfterWriting], &candidate)
            .unwrap_err();

        let PublishError::BundleInvalid {
            refusal: BundleRefusal::NotABundle(report),
        } = &error
        else {
            panic!("expected an invalid staged bundle, got {error}");
        };
        assert!(!report.changed.is_empty(), "{report:?}");
        assert_eq!(
            fixture.observe(identity_of(&candidate), &[]),
            Observed {
                reference: None,
                bundle: BundleAtFinalPath::Absent,
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn a_move_that_truly_failed_publishes_nothing() {
        // Commit point 1 did not pass and the destination is what proves it: no bundle
        // exists there, so the pointer must not move and the attempt is not kept.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let error = fixture
            .publish_failing_at(&[Step::Move], &candidate)
            .unwrap_err();

        assert!(
            matches!(error, PublishError::BundleNotCompleted { .. }),
            "{error}"
        );
        assert_eq!(
            fixture.observe(identity_of(&candidate), &[]),
            Observed {
                reference: None,
                bundle: BundleAtFinalPath::Absent,
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn a_move_that_succeeded_but_reported_failure_is_a_completed_bundle() {
        // The one ambiguous step, decided the way `state` decides its own
        // (src/state/replace.rs:161-183): the destination decides, never the error. The
        // move did happen, so commit point 1 passed and the publication finishes — and the
        // absent staging directory is the second observation that tells this apart from a
        // destination that was already complete before the attempt began.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let published = fixture
            .publish_failing_at(&[Step::MoveThenReportFailure], &candidate)
            .unwrap();

        assert_eq!(published.bundle(), BundleCompletion::Created);
        assert_eq!(published.pointer(), PointerCommit::Committed);
        assert_eq!(
            fixture.observe(published.revision(), &[]),
            Observed {
                reference: Some(published.revision().into()),
                bundle: BundleAtFinalPath::ThisRevision,
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn a_move_that_failed_onto_a_destination_someone_else_completed_is_an_adoption() {
        // The other half of the ambiguous step, and the reason the classification looks at
        // two things rather than one. The destination holds this revision's bundle either
        // because the move landed or because someone else got there first, and those are
        // indistinguishable from the destination alone. The surviving staging directory is
        // what tells them apart: a rename that moved the tree leaves no source behind.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let published = fixture
            .publish_failing_at(&[Step::DestinationCompletedByAnotherWriter], &candidate)
            .unwrap();

        assert_eq!(published.bundle(), BundleCompletion::AlreadyComplete);
        assert_eq!(published.pointer(), PointerCommit::Committed);
        assert_eq!(
            fixture.observe(published.revision(), &[]),
            Observed {
                reference: Some(published.revision().into()),
                bundle: BundleAtFinalPath::ThisRevision,
                // Adopted, so this attempt's own directory was discarded rather than left
                // beside the bundle it did not create.
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn an_unconfirmed_flush_of_the_revisions_directory_refuses_the_pointer_commit() {
        // Commit point 1 passed and the pointer was withheld: the bundle is complete at its
        // final path and unreferenced. Deliberately asymmetric with `state`, which floors
        // at `CommittedDurabilityUncertain` — committing here would take a further
        // irreversible action on an unconfirmed foundation, because a pointer naming a
        // bundle a crash can still erase makes reads fail closed and breaks retention's
        // assumption that a pointer names a complete bundle.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let error = fixture
            .publish_failing_at(&[Step::FlushRevisionsDirectory], &candidate)
            .unwrap_err();

        assert!(
            matches!(error, PublishError::BundleDurabilityUnconfirmed { .. }),
            "{error}"
        );
        let revision = identity_of(&candidate);
        assert_eq!(
            fixture.observe(revision, &[]),
            Observed {
                reference: None,
                bundle: BundleAtFinalPath::ThisRevision,
                staging_attempts: 0,
            }
        );

        // And the stated cost is exactly one rebuild: it adopts the bundle, flushes the
        // directory entry that was never confirmed, and finishes the publication.
        let published = fixture.publish(&candidate).unwrap();
        assert_eq!(published.bundle(), BundleCompletion::AlreadyComplete);
        assert_eq!(published.pointer(), PointerCommit::Committed);
        assert_eq!(
            fixture.observe(revision, &[]),
            Observed {
                reference: Some(revision.into()),
                bundle: BundleAtFinalPath::ThisRevision,
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn a_state_replacement_that_failed_before_its_commit_point_leaves_the_bundle_unreferenced() {
        // The pointer side of a crash between the two commit points, driven through a real
        // `StateStore`: `state`'s replacement failed before its own commit point, so it
        // reports the prior retained and `revisions` must not claim a publication. The
        // stated outcome is a complete unreferenced bundle while readers stay where they
        // were (docs/technical-design.md, "Revision bundles").
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        fixture.state_script.lock().unwrap().fail_rename = true;
        let error = fixture.publish(&candidate).unwrap_err();

        let PublishError::PointerNotCommitted(PublicationError::Mutation(
            MutationError::StorageFailed { .. },
        )) = &error
        else {
            panic!("expected a refused pointer commit, got {error}");
        };
        assert_eq!(
            fixture.observe(identity_of(&candidate), &[]),
            Observed {
                reference: None,
                bundle: BundleAtFinalPath::ThisRevision,
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn an_unconfirmed_state_flush_publishes_with_durability_uncertain() {
        // Not a failure, and the asymmetry with step 8 above is the point: `state` already
        // reopened and validated the authoritative path, so the new revision *is* what a
        // reader sees and re-deciding here would make `revisions` a second authority on
        // which revision is published. The variant survives into the outcome because a
        // retention sweep reads it to know the superseded bundle is not yet retirable.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        fixture.state_script.lock().unwrap().fail_sync_dir = true;
        let published = fixture.publish(&candidate).unwrap();

        assert_eq!(published.bundle(), BundleCompletion::Created);
        assert_eq!(
            published.pointer(),
            PointerCommit::CommittedDurabilityUncertain
        );
        assert_eq!(
            fixture.observe(published.revision(), &[]),
            Observed {
                reference: Some(published.revision().into()),
                bundle: BundleAtFinalPath::ThisRevision,
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn a_wrong_expected_prior_leaves_a_complete_unreferenced_bundle() {
        // The compare-and-swap refusing is a legitimate outcome, not an error to paper
        // over: another writer published in between, so this publication's view of what it
        // was replacing is stale. Readers continue on the previous revision — here, on none
        // — while a complete bundle sits unreferenced.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let error = fixture
            .publish_expecting(
                &candidate,
                Some(&RevisionId(
                    "a revision that was never published".to_owned(),
                )),
            )
            .unwrap_err();

        let PublishError::PointerNotCommitted(_) = &error else {
            panic!("expected a refused pointer commit, got {error}");
        };
        // `state` owns the outcome space, so the refusal is reachable as a source rather
        // than re-discriminated into variants here.
        assert!(
            std::error::Error::source(&error)
                .and_then(|source| source.downcast_ref::<PublicationError>())
                .is_some(),
            "{error}"
        );
        assert_eq!(
            fixture.observe(identity_of(&candidate), &[]),
            Observed {
                reference: None,
                bundle: BundleAtFinalPath::ThisRevision,
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn an_identical_bundle_already_at_the_final_path_is_adopted() {
        // The recovery path after a crash between the commit points, planted directly
        // because the protocol cannot tell its own stranded work from a restored backup by
        // provenance — only by content. Without adoption the revision could never be
        // published again, since a directory rename refuses an occupied destination
        // (ENOTEMPTY). It is sound because equal identifiers mean equal manifest bodies,
        // `unexpected` proves the directory holds nothing besides what that manifest names,
        // and `validate_as` proves the manifest is this revision's.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let revision = identity_of(&candidate);
        fixture.plant_bundle(revision, &candidate);

        let published = fixture.publish(&candidate).unwrap();

        assert_eq!(published.revision(), revision);
        assert_eq!(published.bundle(), BundleCompletion::AlreadyComplete);
        assert_eq!(published.pointer(), PointerCommit::Committed);
        assert_eq!(
            fixture.observe(revision, &[]),
            Observed {
                reference: Some(revision.into()),
                bundle: BundleAtFinalPath::ThisRevision,
                // One bundle, not two: this attempt's own staging was discarded rather
                // than left beside the one it adopted.
                staging_attempts: 0,
            }
        );
        assert_eq!(entries(&bundles_root(&fixture.root)).len(), 1);
    }

    #[test]
    fn a_corrupt_bundle_at_the_final_path_is_refused_with_the_attempt_retained() {
        // The occupant is never deleted: a pinned reader may hold it, and repair is a
        // retention concern for a later phase (docs/technical-design.md, "Revision
        // bundles"). The staging directory is the one exception to the removal rule — it is
        // the only evidence of the intended replacement — and the revision published before
        // this one is untouched, which is what "readers stay where they were" means here.
        let fixture = Fixture::new();
        let first = fixture.publish(&fixture.candidate("tech_a")).unwrap();
        let candidate = fixture.candidate("tech_b");
        let revision = identity_of(&candidate);
        let occupant = bundle_path(&fixture.root, revision);
        fs::create_dir_all(&occupant).unwrap();
        fs::write(occupant.join("half-written.json"), b"{").unwrap();

        let error = fixture
            .publish_expecting(&candidate, Some(&first.revision().into()))
            .unwrap_err();

        let PublishError::PublishedBundleUnusable {
            refusal: BundleRefusal::NotABundle(report),
        } = &error
        else {
            panic!("expected a damaged occupant, got {error}");
        };
        // No manifest, so nothing else about the directory is even knowable — which is
        // why this cannot be repaired into an adoption and must fail closed.
        assert!(report.manifest_unreadable.is_some(), "{report:?}");
        // Untouched, byte for byte.
        assert_eq!(fs::read(occupant.join("half-written.json")).unwrap(), b"{");
        assert_eq!(
            // `damaged`: this row planted the occupant, so the reader invariant cannot
            // hold for it. That it does not hold is the finding, not a gap.
            fixture.observe(revision, &[&revision.to_hex()]),
            Observed {
                reference: Some(first.revision().into()),
                bundle: BundleAtFinalPath::NotThisRevision,
                staging_attempts: 1,
            }
        );
    }

    #[test]
    fn a_valid_bundle_of_another_revision_at_the_final_path_is_refused_rather_than_adopted() {
        // Adoption assumes that a directory under `bundles/` is named by its own manifest's
        // identifier. That is true only because this protocol derives the name from the
        // sealed manifest and is the sole writer there — and "only one writer exists right
        // now" is not an invariant. Without `validate_as`, this bundle would satisfy
        // `validate`, be adopted, and be published as a revision it is not: the pointer
        // would name a complete, self-consistent bundle of the wrong content.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_b");
        let revision = identity_of(&candidate);
        let impostor = fixture.candidate("tech_a");
        fixture.plant_bundle(revision, &impostor);

        let error = fixture.publish(&candidate).unwrap_err();

        let PublishError::PublishedBundleUnusable { refusal } = &error else {
            panic!("expected the occupant to be refused, got {error}");
        };
        // Refused as another revision, not as damage: the distinction is the point, since
        // a damaged occupant invites repair and this one must simply be left alone.
        assert_eq!(
            refusal,
            &BundleRefusal::AnotherRevision(RevisionMismatch {
                expected: revision,
                found: identity_of(&impostor),
            })
        );
        // Never deleted, and never repaired into place: it is still exactly the bundle it
        // was, which is what a pinned reader of *that* revision would still need.
        assert!(validate(&bundle_path(&fixture.root, revision)).is_ok());
        assert_eq!(
            fixture.observe(revision, &[&revision.to_hex()]),
            Observed {
                reference: None,
                bundle: BundleAtFinalPath::NotThisRevision,
                // Retained, for the same reason as a damaged occupant: the intended
                // replacement has no other evidence.
                staging_attempts: 1,
            }
        );
    }

    #[test]
    fn a_cleanup_that_cannot_remove_an_abandoned_attempt_does_not_change_the_outcome() {
        // Removal is best-effort by design, and this is what that costs and why it is
        // affordable: the outcome is the staging failure, not the cleanup failure, and the
        // directory left behind is recognizable as abandoned staging by name, so the
        // retention sweep finds it in a later process (stage.rs, `is_staging_attempt_name`).
        // Failing a publication over a failed cleanup would trade a recoverable directory
        // for an unrecoverable refusal.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let error = fixture
            .publish_failing_at(&[Step::WriteManifest, Step::DiscardStaging], &candidate)
            .unwrap_err();

        staging_failed_at(&error, Step::WriteManifest);
        assert_eq!(
            fixture.observe(identity_of(&candidate), &[]),
            Observed {
                reference: None,
                bundle: BundleAtFinalPath::Absent,
                staging_attempts: 1,
            }
        );
        assert!(crate::revisions::stage::is_staging_attempt_name(
            &entries(&staging_root(&fixture.root))[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
        ));
    }

    #[test]
    fn publication_changes_only_the_publication_reference() {
        // The enforcement for "`revisions` cannot modify unrelated settings" (D-090). The
        // capability makes the rest of the store inexpressible; this proves the one
        // mutation it *can* express is also narrow in effect. The unresolved-quarantine
        // notice is the sharpest case — it gates orphan cleanup, and a publication that
        // silently cleared it would re-enable deleting artifacts whose references are
        // still only recoverable from the quarantined bytes (D-109).
        let fixture = Fixture::opened(|state_dir| {
            fs::write(state_dir.join("state.json"), b"{not json").unwrap();
        });
        let (other_location, other_installation) =
            install(&fixture.pointer.store, "/tmp/other", "ugc_2");
        // Through the capability, not through the store: `StateStore`'s compare-and-swap is
        // visible only inside `state`, so this is the same route the protocol takes.
        fixture
            .pointer
            .capability()
            .set_publication_reference(
                other_installation,
                other_location,
                None,
                RevisionId("another installation's revision".to_owned()),
            )
            .unwrap();

        let before = fixture.pointer.store.snapshot();
        assert!(before.unresolved_quarantine.is_some());
        let published = fixture.publish(&fixture.candidate("tech_a")).unwrap();
        let after = fixture.pointer.store.snapshot();

        assert_eq!(after.schema, before.schema);
        assert_eq!(after.discovery_locations, before.discovery_locations);
        assert_eq!(after.unresolved_quarantine, before.unresolved_quarantine);
        // Exactly one entry differs, and it is this installation's: removing it from both
        // sides leaves two byte-identical maps.
        assert_eq!(
            after.publication_references[&fixture.pointer.installation].revision,
            published.revision().into()
        );
        let mut remaining = after.publication_references.clone();
        remaining.remove(&fixture.pointer.installation);
        assert_eq!(remaining, before.publication_references);
        // The other installation's reference is deliberately not a bundle under this root,
        // so the reader invariant is asked only about the one this publication wrote.
        every_directory_under_bundles_always_validates(
            &fixture.root,
            Some(&after.publication_references[&fixture.pointer.installation].revision),
            &[],
        );
    }

    #[test]
    fn republishing_an_unchanged_candidate_reaches_the_same_bundle() {
        // Idempotence is not a nicety here: a rebuild is the protocol's own recovery step,
        // so an unchanged candidate must converge on the revision that is already
        // published rather than accumulate a second bundle beside it. This is the same
        // adoption path a stranded attempt takes, reached from a *completed* publication.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let first = fixture.publish(&candidate).unwrap();
        let again = fixture
            .publish_expecting(&candidate, Some(&first.revision().into()))
            .unwrap();

        assert_eq!(again.revision(), first.revision());
        assert_eq!(again.bundle(), BundleCompletion::AlreadyComplete);
        assert_eq!(again.pointer(), PointerCommit::Committed);
        assert_eq!(entries(&bundles_root(&fixture.root)).len(), 1);
        assert_eq!(
            fixture.observe(first.revision(), &[]),
            Observed {
                reference: Some(first.revision().into()),
                bundle: BundleAtFinalPath::ThisRevision,
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn candidates_differing_only_in_content_publish_to_different_bundles() {
        // The other half of idempotence, and what makes a published bundle immutable in
        // practice: changed content is a different revision at a different path, never an
        // edit of the one already there. Both remain complete and only the newer is named.
        let fixture = Fixture::new();
        let first = fixture.publish(&fixture.candidate("tech_a")).unwrap();
        let second = fixture
            .publish_expecting(&fixture.candidate("tech_b"), Some(&first.revision().into()))
            .unwrap();

        assert_ne!(first.revision(), second.revision());
        assert_eq!(second.bundle(), BundleCompletion::Created);
        assert_eq!(entries(&bundles_root(&fixture.root)).len(), 2);
        assert_eq!(
            bundle_at(&fixture.root, first.revision()),
            BundleAtFinalPath::ThisRevision
        );
        assert_eq!(
            fixture.observe(second.revision(), &[]),
            Observed {
                reference: Some(second.revision().into()),
                bundle: BundleAtFinalPath::ThisRevision,
                staging_attempts: 0,
            }
        );
    }

    #[test]
    fn opening_puts_staging_beside_bundles_and_survives_being_opened_twice() {
        // Adjacency on one filesystem is a precondition of the protocol, not a
        // convenience: it is what makes the move a rename rather than a cross-device copy
        // (docs/technical-design.md, "Revision bundles"). Creating both here is what makes
        // it structural. Opening twice is the ordinary case — the second launch is not the
        // first — so it must not fail because the directories already exist.
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("revisions");
        drop(RevisionStore::open(&root).unwrap());
        drop(RevisionStore::open(&root).unwrap());
        assert!(bundles_root(&root).is_dir());
        assert!(staging_root(&root).is_dir());
        assert_eq!(bundles_root(&root).parent(), staging_root(&root).parent());
    }

    #[cfg(feature = "test-support")]
    #[test]
    fn a_revision_built_from_a_fixture_corpus_is_reachable_through_the_state_pointer() {
        // The whole thread end to end, with real input identity rather than a hand-made
        // digest: a `FixtureCorpus` snapshot's `SourceFingerprint` — derived by the same
        // construction and fingerprint path a live source uses (D-113) — reaches the sealed
        // manifest, survives the move and the pointer commit, and comes back out of a
        // bundle resolved *only* from the state pointer. That last step is the shape
        // STE-18's acceptance harness keeps, and the one STE-17's reader replaces: it opens
        // a bundle by identifier and must confirm the directory is that revision's.
        use crate::revisions::manifest::BundleManifest;
        use crate::source::fixture::FixtureCorpus;
        use crate::source::snapshot::SourceKind;

        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file(
                "descriptor.mod",
                b"name=\"Fixture Mod\"\nsupported_version=\"4.4\"\n",
            )
            .with_file("common/technology/00_tech.txt", b"tech_a = { cost = 1 }\n")
            .build()
            .unwrap();
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file("common/technology/00_tech.txt", b"tech_a = { cost = 2 }\n")
            .build()
            .unwrap();

        let fixture = Fixture::new();
        let candidate = RevisionCandidate::new(
            fixture.pointer.installation,
            RevisionInputs {
                target_mod: target.fingerprint(),
                vanilla_content: vanilla.fingerprint(),
            },
            AnalysisVersionVector::current(),
            true,
            vec![RevisionDocument::EntryList(EntryList {
                entries: vec![EntrySummary {
                    category: "technology".to_owned(),
                    identifier: "tech_a".to_owned(),
                    display_name: Some("Fixture Technology".to_owned()),
                }],
            })],
        )
        .unwrap();

        let published = fixture.publish(&candidate).unwrap();

        // From here nothing remembers what was published: the pointer is the only way in.
        let reference = fixture.pointer.published().unwrap();
        let identity = RevisionIdentity::try_from(&reference).unwrap();
        let bundle = validate_as(&bundle_path(&fixture.root, identity), identity).unwrap();
        assert_eq!(bundle.identity(), published.revision());

        let manifest =
            BundleManifest::parse(&fs::read(bundle.path().join(MANIFEST_NAME)).unwrap()).unwrap();
        assert_eq!(manifest.inputs().target_mod, target.fingerprint());
        assert_eq!(manifest.inputs().vanilla_content, vanilla.fingerprint());
        assert_eq!(manifest.installation(), fixture.pointer.installation);
        assert!(manifest.documentation_is_complete());
        // A different corpus is a different revision, so the fingerprint is load-bearing
        // in the identifier rather than merely recorded beside it.
        assert_ne!(target.fingerprint(), vanilla.fingerprint());
    }

    #[test]
    fn publish_errors_display_and_implement_std_error() {
        // Application and transport call sites `?` and log these; both need Display, and
        // std::error::Error is what makes `?` conversion available (the same rationale as
        // state/mutations.rs:690). Every variant that wraps another error must expose it
        // as a source rather than flattening its message away.
        fn assert_error<E: std::error::Error>(error: &E) -> String {
            error.to_string()
        }

        let fixture = Fixture::new();
        let staging = stage_bundle(
            &mut FailAt {
                steps: vec![Step::WriteManifest],
                revisions_root: fixture.root.clone(),
                inner: RealPublicationIo,
            },
            &fixture.root,
            &fixture.candidate("tech_a"),
        )
        .unwrap_err();
        let damaged = BundleRefusal::NotABundle(ValidationReport {
            identifier_mismatch: true,
            ..ValidationReport::default()
        });
        let misfiled = BundleRefusal::AnotherRevision(RevisionMismatch {
            expected: identity_of(&fixture.candidate("tech_a")),
            found: identity_of(&fixture.candidate("tech_b")),
        });

        for (error, expected, has_source) in [
            (
                PublishError::StagingFailed(staging),
                "injected WriteManifest failure",
                true,
            ),
            (
                PublishError::BundleInvalid {
                    refusal: damaged.clone(),
                },
                "identifier",
                true,
            ),
            (
                PublishError::PublishedBundleUnusable { refusal: damaged },
                "identifier",
                true,
            ),
            (
                PublishError::PublishedBundleUnusable { refusal: misfiled },
                "expected here",
                true,
            ),
            (
                PublishError::BundleNotCompleted {
                    detail: "the move was refused".to_owned(),
                },
                "the move was refused",
                false,
            ),
            (
                PublishError::BundleDurabilityUnconfirmed {
                    detail: "the flush was refused".to_owned(),
                },
                "the flush was refused",
                false,
            ),
            (
                PublishError::PointerNotCommitted(PublicationError::ExpectedMismatch {
                    actual: None,
                }),
                "expected prior",
                true,
            ),
        ] {
            let rendered = assert_error(&error);
            assert!(
                rendered.contains(expected),
                "{expected} missing from {rendered}"
            );
            assert!(!rendered.contains("PublishError"), "{rendered}");
            assert_eq!(
                std::error::Error::source(&error).is_some(),
                has_source,
                "{rendered}"
            );
        }
    }

    #[test]
    fn a_written_file_lands_with_its_parents_created() {
        // `stage_bundle` writes `documents/entry-list.json` into a directory that holds
        // only itself, so the seam has to create the intermediate directory; a caller that
        // had to pre-create each one would be restating bundle layout outside `stage`.
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("documents").join("entry-list.json");
        RealPublicationIo.write_file(&nested, b"{}").unwrap();
        assert_eq!(fs::read(&nested).unwrap(), b"{}");
    }

    #[test]
    fn writing_over_an_existing_file_replaces_its_whole_content() {
        // `fs::File::create` truncates. Without that, rewriting a shorter document into a
        // reused path would leave the previous document's tail behind and the bundle would
        // hash as `changed` for reasons no reader could explain.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("entry.json");
        RealPublicationIo
            .write_file(&path, b"a much longer previous document")
            .unwrap();
        RealPublicationIo.write_file(&path, b"{}").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{}");
    }

    #[test]
    fn creating_a_directory_that_already_exists_succeeds() {
        // Documented as idempotent because `staging/` and `bundles/` are shared and
        // long-lived: the first publication creates them and every later one must not
        // fail because it was not the first.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("staging");
        RealPublicationIo.create_dir(&path).unwrap();
        RealPublicationIo.create_dir(&path).unwrap();
        assert!(path.is_dir());
    }

    #[test]
    fn a_directory_rename_moves_the_whole_subtree_and_refuses_an_occupied_destination() {
        // The move step's real semantics, pinned here rather than assumed by the protocol:
        // the subtree arrives intact, and an occupied destination is an error rather than
        // a silent replacement. Task 6 depends on both — the first is what makes the
        // bundle complete at one instant, the second is why an already-present bundle is
        // adopted after validation instead of overwritten.
        let dir = TempDir::new().unwrap();
        let from = dir.path().join("staging").join("attempt");
        RealPublicationIo
            .write_file(&from.join("documents").join("entry-list.json"), b"{}")
            .unwrap();
        let to = dir.path().join("bundles").join("abc");
        RealPublicationIo.create_dir(to.parent().unwrap()).unwrap();
        RealPublicationIo.rename(&from, &to).unwrap();
        assert!(!from.exists());
        assert_eq!(
            fs::read(to.join("documents").join("entry-list.json")).unwrap(),
            b"{}"
        );

        let second = dir.path().join("staging").join("other-attempt");
        RealPublicationIo
            .write_file(&second.join("manifest.json"), b"{}")
            .unwrap();
        assert!(RealPublicationIo.rename(&second, &to).is_err());
        assert!(second.exists());
    }

    #[test]
    fn removing_an_abandoned_staging_directory_removes_its_contents() {
        let dir = TempDir::new().unwrap();
        let abandoned = dir.path().join("attempt");
        RealPublicationIo
            .write_file(&abandoned.join("documents").join("half.json"), b"{")
            .unwrap();
        RealPublicationIo.remove_dir_all(&abandoned).unwrap();
        assert!(!abandoned.exists());
    }

    #[test]
    fn syncing_a_directory_that_does_not_exist_reports_the_failure() {
        // `sync_dir` is a durability step, not a best-effort one: the protocol reads its
        // result, so it must not swallow a path that was never created.
        let dir = TempDir::new().unwrap();
        assert!(
            RealPublicationIo
                .sync_dir(&dir.path().join("absent"))
                .is_err()
        );
    }
}
