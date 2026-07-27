//! Sole owner of Documentation Revision bundle I/O: staging, validation, and atomic
//! publication now; the Revision Reader, handle pinning, and retention from STE-17
//! (docs/technical-design.md, "Documentation revision publication"). Populated in Phase 3.
//!
//! [`RevisionStore::publish`] is the whole product surface: one operation that takes a
//! [`RevisionCandidate`] and either publishes it or reports what is true on disk instead.
//! Staging, the bundle layout, and the validator are deliberately unreachable from
//! outside. A caller able to stage without publishing, or to move a directory itself,
//! could produce exactly the intermediate states the protocol exists to make unreachable,
//! and the module would be holding a rule rather than enforcing one.
//!
//! # What a publication establishes
//!
//! Two commit points, in this order: the durable move of a validated bundle onto its final
//! path, and the compare-and-swap of the state publication reference. Whatever step fails
//! or is interrupted, a reader is left on the previously published revision or on the
//! replacement and never on staging state. [`Published`] reports how far each commit point
//! got — [`PointerCommit::CommittedDurabilityUncertain`] is a success that withholds
//! retirement eligibility, not a partial failure — and every [`PublishError`] variant names
//! what is true on disk when it is returned.
//!
//! The layout is hidden behind that. A revision is addressed by its [`RevisionIdentity`]
//! and no value this module returns carries a filesystem path, so nothing outside acquires
//! the ability to open bundle files ahead of the reader STE-17 adds.
//!
//! # What a caller may not assume
//!
//! **That holding a [`RevisionStore`] excludes anybody.** Its lock serializes this
//! process's protocol I/O against one revisions root and nothing more; the build lease that
//! makes "one active documentation build" true arrives in Phase 9.
//!
//! **That publication re-checks the source.** The candidate's fingerprints are recorded,
//! never re-observed. Verifying that the live source still matches immediately before
//! publishing is the build coordinator's step, and `revisions` does not silently acquire it
//! (D-085): a candidate built from a tree that has since changed publishes without
//! complaint.
//!
//! **That a refusal can be retried by looping.** A stale `expected_prior` is reported as
//! `ExpectedMismatch` *after* the bundle has reached its final path, so a retry that only
//! re-reads the pointer republishes the same identifier — finds its own bundle already
//! complete — and races the same way again. The caller re-derives its intent or gives up.
//!
//! # Platform caveat: a directory flush is not universally available
//!
//! Both commit points rest on a directory's entries reaching disk, and not every platform
//! and filesystem provides an operation that means that. Windows needs a handle opened with
//! `FILE_FLAG_BACKUP_SEMANTICS` and carrying `GENERIC_WRITE`, and some redirectors answer
//! "incorrect function" even then. The decision, the two Win32 contracts it quotes, and the
//! residual risk are homed once in [`durability`](crate::durability) and recorded as D-123,
//! because [`state::replace`](crate::state::replace) commits against the same platform fact.
//!
//! What it means here: step 8's refusal — [`PublishError::BundleDurabilityUnconfirmed`], the
//! D-121 behaviour — is reachable only from a flush that was attempted and failed, never
//! from the platform having nothing to offer. Without that qualifier no revision could ever
//! be published where the operation is unavailable. The converse matters just as much and is
//! the correction D-123 records: the tolerance must stay narrow enough that this refusal is
//! still *reachable* on Windows, or the module would be claiming a durability check it never
//! performs.
//!
//! # Platform caveat: an open handle can refuse a directory rename
//!
//! Commit point 1 moves a whole directory. On Windows a rename is refused with a sharing
//! violation while a handle inside the tree is open, rather than proceeding as it does
//! under POSIX. Nothing outside this module can hold one today — a staging directory's name
//! is a fresh UUID that leaves only inside a [`PublishError`] — but STE-17's Revision Reader
//! pins handles inside `bundles/`, which is where retention's removals will meet the same
//! restriction, and where "publication is never blocked by a reader" stops being free.
//! Established on a real machine by the Phase 12 Windows packaged smoke test, the way
//! [`state::replace`](crate::state::replace) records the equivalent caveat for its own
//! replacement semantics.

pub mod candidate;
pub mod manifest;
mod publish;
mod stage;

pub use candidate::{
    CandidateError, DocumentIdentity, EntryList, EntrySummary, RevisionCandidate, RevisionDocument,
    RevisionInputs,
};
pub use manifest::{
    BUNDLE_SCHEMA_VERSION, BundleManifest, IdentityParseError, ManifestParseError, RevisionIdentity,
};
pub use publish::{BundleCompletion, PointerCommit, PublicationIo, PublishError, Published};
pub use stage::{
    BundleRefusal, RevisionMismatch, SchemaMismatch, StageError, ValidationReport,
    is_staging_attempt_name,
};

use publish::{RealPublicationIo, publish_revision};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::discovery::identity::DiscoveryLocationId;
use crate::state::{PublicationCapability, RevisionId};

/// The revisions location, and the publication protocol that owns it.
///
/// Every publication runs through [`RevisionStore::publish`]: application use cases supply
/// a validated Revision Candidate and never separately move a bundle or mutate state
/// (docs/technical-design.md, "Revision bundles").
///
/// **Not the build lease.** The mutex serializes this process's protocol I/O so two
/// publications cannot interleave their steps against one revisions root. It is not the
/// Phase 9 build lease and must not be mistaken for one: it excludes no other process, it
/// is not held across a build, and holding it grants no right to produce a revision. A
/// caller that needs those guarantees waits for the lease Phase 9 introduces.
pub struct RevisionStore {
    root: PathBuf,
    io_seam: Mutex<Box<dyn PublicationIo + Send>>,
}

impl RevisionStore {
    /// Opens the revisions location, creating `bundles/` and `staging/` under it.
    ///
    /// Creating both is a precondition of the protocol rather than a convenience: staging
    /// is adjacent to bundles on the same filesystem, which is what makes the move a
    /// rename and not a cross-device copy (docs/technical-design.md, "Revision bundles").
    ///
    /// Both are flushed into `root` before this returns, so the directories a publication
    /// commits into are durable before anything is published into them. **The entry naming
    /// `root` itself inside the application-data directory is the caller's**, not this
    /// module's: the composition root owns that directory, and a chain of flushes that
    /// walked upward from here would have no principled stopping point short of the volume.
    ///
    /// The error is a plain [`io::Error`]. Open makes exactly these two directories and has
    /// no classification of its own to add; a typed wrapper would only restate the cause.
    pub fn open(root: &Path) -> io::Result<Self> {
        Self::open_seamed(root, Box::new(RealPublicationIo))
    }

    /// Publication-I/O seam for tests only. Behind the `test-support` feature so it cannot
    /// reach a shipped binary, mirroring [`StateStore::open_with_io`] (docs/decision-log.md,
    /// D-107).
    ///
    /// [`StateStore::open_with_io`]: crate::state::StateStore::open_with_io
    #[cfg(any(test, feature = "test-support"))]
    pub fn open_with_io(root: &Path, io_seam: Box<dyn PublicationIo + Send>) -> io::Result<Self> {
        Self::open_seamed(root, io_seam)
    }

    fn open_seamed(root: &Path, mut io_seam: Box<dyn PublicationIo + Send>) -> io::Result<Self> {
        io_seam.create_dir(&stage::bundles_root(root))?;
        io_seam.create_dir(&stage::staging_root(root))?;
        // And flush the root, which is the entry that names them. Step 8 of every
        // publication flushes `bundles/`, which makes a `<hex>` entry durable *inside*
        // `bundles/`; it cannot make `bundles/`'s own entry inside `<root>` durable. Without
        // this, the first publication after these directories are created can commit its
        // pointer, crash, and come back to a root that no longer contains `bundles/` at all
        // — a reference naming a revision whose whole tree is gone, which is precisely the
        // outcome the two commit points exist to make unreachable. It is the same
        // deepest-first chain `stage_bundle` follows, continued to the top rather than
        // stopped one level short.
        //
        // Failing here is the cheap place to fail: nothing has been published yet, so the
        // refusal costs a retry rather than a revision.
        io_seam.sync_dir(root)?;
        Ok(Self {
            root: root.to_path_buf(),
            io_seam: Mutex::new(io_seam),
        })
    }

    /// Runs the publication protocol for one candidate: see [`publish_revision`] for the
    /// steps and the two commit points.
    ///
    /// `location` is separate from the candidate because it is configuration rather than
    /// identity: a Discovery Location's path is editable and a rebind must not invalidate a
    /// revision, so it never enters the manifest (docs/technical-design.md, "Installation
    /// identity"). It reaches only the state pointer.
    ///
    /// `expected_prior` comes from the caller for the same reason the capability does:
    /// `revisions` never reads the pointer, and the caller is the only place that knows
    /// which revision it intends to replace.
    pub fn publish(
        &self,
        candidate: &RevisionCandidate,
        location: DiscoveryLocationId,
        expected_prior: Option<&RevisionId>,
        publication: &PublicationCapability<'_>,
    ) -> Result<Published, PublishError> {
        let mut io_seam = self.lock();
        publish_revision(
            io_seam.as_mut(),
            &self.root,
            candidate,
            location,
            expected_prior,
            publication,
        )
    }

    /// Recovers from poisoning rather than propagating it, deliberately and for a narrower
    /// reason than [`StateStore::lock`](crate::state::StateStore): the guarded value is the
    /// I/O seam alone. It carries no protocol state — every decision of a publication lives
    /// in local variables of one [`publish`](RevisionStore::publish) call, and what a
    /// crashed attempt left on disk is reclassified from disk rather than remembered. A
    /// panic elsewhere in the process therefore leaves nothing torn here, while refusing
    /// the lock for the process lifetime would turn an unrelated panic into permanent
    /// inability to publish.
    fn lock(&self) -> MutexGuard<'_, Box<dyn PublicationIo + Send>> {
        self.io_seam.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
