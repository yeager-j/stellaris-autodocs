//! Sole owner of Documentation Revision bundle I/O: staging, validation, atomic
//! publication, the Revision Reader, handle pinning, and retention
//! (docs/technical-design.md, "Documentation revision publication"). Populated in Phase 3.

pub mod candidate;
pub mod manifest;
pub mod publish;
pub mod stage;

use candidate::RevisionCandidate;
use publish::{PublicationIo, PublishError, Published, RealPublicationIo, publish_revision};
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
