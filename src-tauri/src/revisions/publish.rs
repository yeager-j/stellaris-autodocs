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
    StageError, ValidationReport, bundle_path, bundles_root, stage_bundle, validate,
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
    /// The staged bundle disagreed with its own manifest before anything was moved. The
    /// staging directory was removed best-effort: it is damaged evidence of a build this
    /// process can simply repeat, and the report is the diagnostic.
    BundleInvalid { report: ValidationReport },
    /// The final path is occupied by something that is not a valid bundle. Nothing was
    /// moved, the pointer is unchanged, and **the staging directory is retained**: it is
    /// the only evidence of the intended replacement. The occupant is never deleted — a
    /// pinned reader may hold it, and repair is a retention concern for a later phase
    /// (docs/technical-design.md, "Revision bundles").
    PublishedBundleCorrupt { report: ValidationReport },
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
            Self::BundleInvalid { report } => {
                write!(f, "the staged revision was not moved: {report}")
            }
            Self::PublishedBundleCorrupt { report } => write!(
                f,
                "a damaged revision already occupies this revision's path and was left \
                 untouched: {report}"
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
            Self::BundleInvalid { report } | Self::PublishedBundleCorrupt { report } => {
                Some(report)
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
    // what only `validate` constructs.
    let validated = match validate(staged.root()) {
        Ok(validated) => validated,
        Err(report) => {
            discard_staging(io_seam, staged.root());
            return Err(PublishError::BundleInvalid { report });
        }
    };

    // Step 7 ★. An occupied final path is never renamed onto: a directory rename refuses a
    // non-empty destination (ENOTEMPTY) rather than replacing it, so what the destination
    // already holds decides the outcome before the move is attempted.
    let bundle = match observe_final_path(&final_path) {
        FinalPath::Complete => {
            // The same bundle is already there — an identical rebuild after a crash
            // between the commit points. Adoption is sound because equal identifiers mean
            // equal manifest bodies and validation's `unexpected` check proves the
            // directory holds nothing besides what that manifest names.
            //
            // It assumes what only this protocol can establish: that a directory under
            // `bundles/` is named by the identifier its own manifest carries, because
            // nothing else writes there and the name is always derived from the sealed
            // manifest. `validate` proves a directory is *a* bundle, not that it is *this*
            // revision's, so a bundle hand-copied to another revision's path would be
            // adopted as that revision. STE-17's reader is where a published bundle is
            // opened by identifier and is the natural owner of that confirmation.
            discard_staging(io_seam, staged.root());
            BundleCompletion::AlreadyComplete
        }
        FinalPath::Corrupt(report) => {
            // Staging is deliberately kept: it is the only evidence of the intended
            // replacement, and the occupant is never deleted.
            return Err(PublishError::PublishedBundleCorrupt { report });
        }
        FinalPath::Absent => match io_seam.rename(validated.path(), &final_path) {
            Ok(()) => BundleCompletion::Created,
            Err(error) => classify_reported_rename_failure(
                io_seam,
                staged.root(),
                &final_path,
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
    Complete,
    Corrupt(ValidationReport),
}

fn observe_final_path(path: &Path) -> FinalPath {
    // Read with plain `std::fs` rather than through the seam, for the reason
    // [`PublicationIo`] records: a read is not a commit point.
    if !path.exists() {
        return FinalPath::Absent;
    }
    match validate(path) {
        Ok(_) => FinalPath::Complete,
        Err(report) => FinalPath::Corrupt(report),
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
    detail: &str,
) -> Result<BundleCompletion, PublishError> {
    match observe_final_path(final_path) {
        FinalPath::Complete if staging.exists() => {
            discard_staging(io_seam, staging);
            Ok(BundleCompletion::AlreadyComplete)
        }
        FinalPath::Complete => Ok(BundleCompletion::Created),
        FinalPath::Corrupt(report) => Err(PublishError::PublishedBundleCorrupt { report }),
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
        bundle_path, bundles_root, stage_bundle, staging_root, validate,
    };
    use crate::source::ObservationGaps;
    use crate::source::fingerprint::{ContentHash, SourceFingerprint};
    use crate::state::replace::{RealIo, ReplacementIo};
    use crate::state::{
        OpenOutcome, PublicationCapability, PublicationError, RevisionId, StateStore,
    };
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// The state store, its one Discovery Location, and the installation a candidate
    /// documents there. Assembled together because `set_publication_reference` refuses an
    /// unknown location, so every publication test needs the pair to be coherent.
    struct Pointer {
        store: StateStore,
        location: DiscoveryLocationId,
        installation: ModInstallationId,
    }

    impl Pointer {
        fn opened(dir: &Path) -> Self {
            fs::create_dir_all(dir).unwrap();
            Self::seamed(StateStore::open(dir).unwrap())
        }

        fn seamed(outcome: OpenOutcome) -> Self {
            let OpenOutcome::Ready { store, .. } = outcome else {
                panic!("state store blocked");
            };
            let (location, _) = store
                .add_discovery_location(PathBuf::from("/tmp/workshop"))
                .unwrap();
            let installation =
                ModInstallationId::derive(location, &LogicalPath::parse("ugc_1").unwrap());
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

    /// A candidate for `pointer`'s installation whose one entry carries `marker`, so two
    /// candidates that differ only in `marker` are two different revisions.
    fn candidate(pointer: &Pointer, marker: &str) -> RevisionCandidate {
        RevisionCandidate::new(
            pointer.installation,
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

    /// Every staging directory left under the revisions root.
    fn staging_attempts(root: &Path) -> Vec<PathBuf> {
        entries(&staging_root(root))
    }

    /// Every published bundle directory under the revisions root.
    fn published_bundles(root: &Path) -> Vec<PathBuf> {
        entries(&bundles_root(root))
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

    /// The one protocol step this seam fails; every other step is real.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Step {
        WriteManifest,
        /// Rewrites a document after `stage_bundle` hashed it, which is what a corrupted
        /// staged bundle looks like from the validator's side.
        CorruptAfterWriting,
        Move,
        /// The ambiguous case: the move happens and the seam reports failure anyway.
        MoveThenReportFailure,
        FlushRevisionsDirectory,
    }

    struct FailAt {
        step: Step,
        revisions_root: PathBuf,
        inner: RealPublicationIo,
    }

    impl FailAt {
        fn seam(step: Step, revisions_root: &Path) -> Box<dyn PublicationIo + Send> {
            Box::new(Self {
                step,
                revisions_root: revisions_root.to_path_buf(),
                inner: RealPublicationIo,
            })
        }
    }

    impl PublicationIo for FailAt {
        fn create_dir(&mut self, path: &Path) -> io::Result<()> {
            self.inner.create_dir(path)
        }

        fn write_file(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
            if self.step == Step::WriteManifest
                && path.file_name().is_some_and(|name| name == "manifest.json")
            {
                return Err(io::Error::other("injected manifest write failure"));
            }
            self.inner.write_file(path, bytes)
        }

        fn sync_dir(&mut self, path: &Path) -> io::Result<()> {
            if self.step == Step::FlushRevisionsDirectory
                && path == bundles_root(&self.revisions_root)
            {
                return Err(io::Error::other(
                    "injected revisions-directory flush failure",
                ));
            }
            if self.step == Step::CorruptAfterWriting
                && path.file_name().is_some_and(|name| name == "documents")
            {
                fs::write(path.join("entry-list.json"), b"tampered after the write").unwrap();
            }
            self.inner.sync_dir(path)
        }

        fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
            match self.step {
                Step::Move => Err(io::Error::other("injected move failure")),
                Step::MoveThenReportFailure => {
                    self.inner.rename(from, to)?;
                    Err(io::Error::other("injected post-move failure report"))
                }
                _ => self.inner.rename(from, to),
            }
        }

        fn remove_dir_all(&mut self, path: &Path) -> io::Result<()> {
            self.inner.remove_dir_all(path)
        }
    }

    /// A revisions root and a state pointer under one temporary directory. Every scenario
    /// below needs both halves, and the two must not share a directory: a state document
    /// beside `bundles/` would make "nothing else is under the revisions root" untestable.
    struct Fixture {
        _dir: TempDir,
        root: PathBuf,
        pointer: Pointer,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let pointer = Pointer::opened(&dir.path().join("state"));
            let root = dir.path().join("revisions");
            Self {
                _dir: dir,
                root,
                pointer,
            }
        }

        fn store(&self) -> RevisionStore {
            RevisionStore::open(&self.root).unwrap()
        }

        fn store_failing_at(&self, step: Step) -> RevisionStore {
            RevisionStore::open_with_io(&self.root, FailAt::seam(step, &self.root)).unwrap()
        }

        fn candidate(&self, marker: &str) -> RevisionCandidate {
            candidate(&self.pointer, marker)
        }

        /// Publishes with real I/O and no expected prior. The starting point for the
        /// scenarios that need a bundle already at its final path.
        fn publish(&self, candidate: &RevisionCandidate) -> Result<Published, PublishError> {
            self.store().publish(
                candidate,
                self.pointer.location,
                None,
                &self.pointer.capability(),
            )
        }
    }

    #[test]
    fn a_clean_publication_completes_the_bundle_and_commits_the_pointer() {
        // The whole chain this ticket exists to prove: a candidate becomes a validated
        // bundle at its canonical path and the state pointer names it
        // (docs/technical-design.md, "Revision bundles"). Both commit points passed, so
        // the outcome names a new bundle and a committed pointer.
        let fixture = Fixture::new();
        let published = fixture.publish(&fixture.candidate("tech_a")).unwrap();

        assert_eq!(published.bundle(), BundleCompletion::Created);
        assert_eq!(published.pointer(), PointerCommit::Committed);
        assert!(validate(&bundle_path(&fixture.root, published.revision())).is_ok());
        assert_eq!(
            fixture.pointer.published(),
            Some(published.revision().into())
        );
        // Nothing was left behind in staging: the attempt's directory was renamed away.
        assert_eq!(staging_attempts(&fixture.root), Vec::<PathBuf>::new());
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

    #[test]
    fn a_staging_failure_publishes_nothing_and_removes_the_partial_directory() {
        // Failure before every commit point: the previously published revision is
        // untouched, no bundle appears, and the partial attempt does not accumulate. The
        // removal rule is `RealIo::write_temp_in`'s (src/state/replace.rs:55-82) — an
        // attempt that will never be renamed is unreachable, and leaving one behind costs
        // the user a directory on every ENOSPC.
        let fixture = Fixture::new();
        let error = fixture
            .store_failing_at(Step::WriteManifest)
            .publish(
                &fixture.candidate("tech_a"),
                fixture.pointer.location,
                None,
                &fixture.pointer.capability(),
            )
            .unwrap_err();

        assert!(matches!(error, PublishError::StagingFailed(_)), "{error}");
        assert_eq!(published_bundles(&fixture.root), Vec::<PathBuf>::new());
        assert_eq!(staging_attempts(&fixture.root), Vec::<PathBuf>::new());
        assert_eq!(fixture.pointer.published(), None);
    }

    #[test]
    fn a_staged_bundle_that_does_not_validate_is_never_moved() {
        // Step 6 stands between staging and the first commit point, and it reads the
        // directory back from disk rather than trusting what was just written: a document
        // rewritten after it was hashed is exactly the shape an in-memory check would
        // miss. Nothing reaches `bundles/`, so no reader can ever observe it.
        let fixture = Fixture::new();
        let error = fixture
            .store_failing_at(Step::CorruptAfterWriting)
            .publish(
                &fixture.candidate("tech_a"),
                fixture.pointer.location,
                None,
                &fixture.pointer.capability(),
            )
            .unwrap_err();

        let PublishError::BundleInvalid { report } = &error else {
            panic!("expected an invalid staged bundle, got {error}");
        };
        assert!(!report.changed.is_empty(), "{report:?}");
        assert_eq!(published_bundles(&fixture.root), Vec::<PathBuf>::new());
        assert_eq!(staging_attempts(&fixture.root), Vec::<PathBuf>::new());
        assert_eq!(fixture.pointer.published(), None);
    }

    #[test]
    fn a_move_that_truly_failed_publishes_nothing() {
        // Commit point 1 did not pass and the destination proves it: no bundle exists, so
        // the pointer must not move and the attempt is not kept.
        let fixture = Fixture::new();
        let error = fixture
            .store_failing_at(Step::Move)
            .publish(
                &fixture.candidate("tech_a"),
                fixture.pointer.location,
                None,
                &fixture.pointer.capability(),
            )
            .unwrap_err();

        assert!(
            matches!(error, PublishError::BundleNotCompleted { .. }),
            "{error}"
        );
        assert_eq!(published_bundles(&fixture.root), Vec::<PathBuf>::new());
        assert_eq!(staging_attempts(&fixture.root), Vec::<PathBuf>::new());
        assert_eq!(fixture.pointer.published(), None);
    }

    #[test]
    fn a_move_that_succeeded_but_reported_failure_is_a_completed_bundle() {
        // The one ambiguous step, decided the way `state` decides its own
        // (src/state/replace.rs:161-183): the destination decides, never the error. The
        // move did happen, so commit point 1 passed and the publication finishes.
        let fixture = Fixture::new();
        let published = fixture
            .store_failing_at(Step::MoveThenReportFailure)
            .publish(
                &fixture.candidate("tech_a"),
                fixture.pointer.location,
                None,
                &fixture.pointer.capability(),
            )
            .unwrap();

        assert_eq!(published.bundle(), BundleCompletion::Created);
        assert_eq!(published.pointer(), PointerCommit::Committed);
        assert!(validate(&bundle_path(&fixture.root, published.revision())).is_ok());
        assert_eq!(
            fixture.pointer.published(),
            Some(published.revision().into())
        );
        assert_eq!(staging_attempts(&fixture.root), Vec::<PathBuf>::new());
    }

    #[test]
    fn an_unconfirmed_flush_of_the_revisions_directory_refuses_the_pointer_commit() {
        // Deliberately asymmetric with `state`, which floors at
        // `CommittedDurabilityUncertain`. Committing the pointer here would take a further
        // irreversible action on an unconfirmed foundation: a pointer naming a bundle a
        // crash can still erase makes reads fail closed and breaks retention's assumption
        // that a pointer names a complete bundle.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let error = fixture
            .store_failing_at(Step::FlushRevisionsDirectory)
            .publish(
                &candidate,
                fixture.pointer.location,
                None,
                &fixture.pointer.capability(),
            )
            .unwrap_err();

        assert!(
            matches!(error, PublishError::BundleDurabilityUnconfirmed { .. }),
            "{error}"
        );
        // The bundle is there and complete; only the publication was withheld.
        assert_eq!(published_bundles(&fixture.root).len(), 1);
        assert!(validate(&published_bundles(&fixture.root)[0]).is_ok());
        assert_eq!(fixture.pointer.published(), None);

        // And the stated cost is one rebuild: it adopts the bundle, flushes the directory
        // entry that was never confirmed, and finishes the publication.
        let published = fixture.publish(&candidate).unwrap();
        assert_eq!(published.bundle(), BundleCompletion::AlreadyComplete);
        assert_eq!(published.pointer(), PointerCommit::Committed);
        assert_eq!(
            fixture.pointer.published(),
            Some(published.revision().into())
        );
    }

    #[test]
    fn a_refused_pointer_commit_leaves_a_complete_unreferenced_bundle() {
        // The design's stated outcome for a crash between the two commit points
        // (docs/technical-design.md, "Revision bundles"), reached here by a compare-and-swap
        // that does not match: readers continue on the previous revision — here, on none —
        // while a complete bundle sits unreferenced. Not an error to paper over.
        let fixture = Fixture::new();
        let error = fixture
            .store()
            .publish(
                &fixture.candidate("tech_a"),
                fixture.pointer.location,
                Some(&RevisionId(
                    "a revision that was never published".to_owned(),
                )),
                &fixture.pointer.capability(),
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
        assert_eq!(published_bundles(&fixture.root).len(), 1);
        assert!(validate(&published_bundles(&fixture.root)[0]).is_ok());
        assert_eq!(fixture.pointer.published(), None);
    }

    #[test]
    fn an_identical_rebuild_adopts_a_stranded_bundle_and_finishes_the_publication() {
        // The recovery path after a crash between the commit points. An identical rebuild
        // derives the identifier the stranded attempt derived, so it finds its own work
        // already at the final path; without adoption that revision could never be
        // published again, because a directory rename refuses an occupied destination.
        // Adoption is sound because validation's `unexpected` check proves the directory
        // holds nothing besides what this manifest names.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let stranded = fixture.store().publish(
            &candidate,
            fixture.pointer.location,
            Some(&RevisionId(
                "a revision that was never published".to_owned(),
            )),
            &fixture.pointer.capability(),
        );
        assert!(stranded.is_err());

        let published = fixture.publish(&candidate).unwrap();
        assert_eq!(published.bundle(), BundleCompletion::AlreadyComplete);
        assert_eq!(published.pointer(), PointerCommit::Committed);
        assert_eq!(
            fixture.pointer.published(),
            Some(published.revision().into())
        );
        // One bundle, not two, and the rebuild's own attempt was discarded rather than
        // left beside it.
        assert_eq!(published_bundles(&fixture.root).len(), 1);
        assert_eq!(staging_attempts(&fixture.root), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_corrupt_bundle_at_the_final_path_is_refused_with_the_attempt_retained() {
        // The occupant is never deleted: a pinned reader may hold it, and repair is a
        // retention concern for a later phase (docs/technical-design.md, "Revision
        // bundles"). The staging directory is the exception to the removal rule — it is
        // the only evidence of the intended replacement.
        let fixture = Fixture::new();
        let candidate = fixture.candidate("tech_a");
        let first = fixture.publish(&candidate).unwrap();
        let occupant = bundle_path(&fixture.root, first.revision());
        let stowaway = occupant.join("stowaway.json");
        fs::write(&stowaway, b"{}").unwrap();

        let error = fixture.publish(&candidate).unwrap_err();

        let PublishError::PublishedBundleCorrupt { report } = &error else {
            panic!("expected a corrupt published bundle, got {error}");
        };
        assert_eq!(report.unexpected, ["stowaway.json"]);
        // Untouched: the occupant keeps its damage, and the pointer keeps naming it.
        assert!(stowaway.exists());
        assert_eq!(fixture.pointer.published(), Some(first.revision().into()));
        // Retained: exactly one attempt, the one this publication staged.
        assert_eq!(staging_attempts(&fixture.root).len(), 1);
    }

    #[test]
    fn an_uncertain_pointer_commit_is_reported_as_a_success() {
        // The pointer is visible, so this is not a failure: `state` already reopened and
        // validated the authoritative path, and re-deciding here would make `revisions` a
        // second authority on which revision is published. The variant is what a later
        // retention sweep reads to know the superseded bundle is not yet retirable.
        struct UnconfirmedStateFlush;
        impl ReplacementIo for UnconfirmedStateFlush {
            fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
                RealIo.write_temp(dir, bytes)
            }
            fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
                RealIo.rename(from, to)
            }
            fn sync_dir(&mut self, _dir: &Path) -> io::Result<()> {
                Err(io::Error::other("injected state flush failure"))
            }
        }

        let dir = TempDir::new().unwrap();
        let state_dir = dir.path().join("state");
        fs::create_dir_all(&state_dir).unwrap();
        let pointer = Pointer::seamed(
            StateStore::open_with_io(&state_dir, Box::new(UnconfirmedStateFlush)).unwrap(),
        );
        let root = dir.path().join("revisions");
        let published = RevisionStore::open(&root)
            .unwrap()
            .publish(
                &candidate(&pointer, "tech_a"),
                pointer.location,
                None,
                &pointer.capability(),
            )
            .unwrap();

        assert_eq!(
            published.pointer(),
            PointerCommit::CommittedDurabilityUncertain
        );
        assert_eq!(pointer.published(), Some(published.revision().into()));
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
                step: Step::WriteManifest,
                revisions_root: fixture.root.clone(),
                inner: RealPublicationIo,
            },
            &fixture.root,
            &fixture.candidate("tech_a"),
        )
        .unwrap_err();
        let report = ValidationReport {
            identifier_mismatch: true,
            ..ValidationReport::default()
        };

        for (error, expected, has_source) in [
            (
                PublishError::StagingFailed(staging),
                "injected manifest write failure",
                true,
            ),
            (
                PublishError::BundleInvalid {
                    report: report.clone(),
                },
                "identifier",
                true,
            ),
            (
                PublishError::PublishedBundleCorrupt { report },
                "identifier",
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
