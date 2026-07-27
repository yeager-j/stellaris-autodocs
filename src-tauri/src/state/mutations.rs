//! Typed mutations on the state store. The public surface is per-owner and narrow:
//! Discovery Location configuration for setup workflows, and the publication-reference
//! compare-and-swap that is the only mutation `revisions` receives
//! (docs/technical-design.md, "Mutable state storage"). No caller gets the document.

use super::model::{AppState, DiscoveryLocation, PublicationReference, RevisionId};
use super::replace::{ReplaceOutcome, replace_state};
use super::store::{Inner, StateStore, encode};
use crate::discovery::identity::{DiscoveryLocationId, ModInstallationId};
use std::path::PathBuf;
use std::sync::MutexGuard;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationCommit {
    Committed,
    /// Visible but durability unconfirmed; surfaced so workflows can report it.
    CommittedDurabilityUncertain,
}

#[derive(Debug)]
pub enum MutationError {
    UnknownLocation,
    /// The store observed an unresolvable replacement outcome; mutation is stopped
    /// until state recovery resolves it.
    RecoveryRequired,
    /// Failure before the commit point: memory and disk both retain the prior state.
    StorageFailed {
        detail: String,
    },
}

#[derive(Debug)]
pub enum PublicationError {
    /// The stored reference did not match the expected prior; nothing changed.
    ExpectedMismatch {
        actual: Option<RevisionId>,
    },
    Mutation(MutationError),
}

impl StateStore {
    pub fn add_discovery_location(
        &self,
        path: PathBuf,
    ) -> Result<(DiscoveryLocationId, MutationCommit), MutationError> {
        let id = DiscoveryLocationId::generate();
        let commit = self.commit_mutation(|state| {
            state
                .discovery_locations
                .push(DiscoveryLocation { id, path });
            Ok(())
        })?;
        Ok((id, commit))
    }

    /// Explicitly a rebind of the same configured location: identity and derived
    /// installation identities are preserved; only the path changes.
    pub fn rebind_discovery_location(
        &self,
        id: DiscoveryLocationId,
        new_path: PathBuf,
    ) -> Result<MutationCommit, MutationError> {
        self.commit_mutation(|state| {
            let location = state
                .discovery_locations
                .iter_mut()
                .find(|l| l.id == id)
                .ok_or(MutationError::UnknownLocation)?;
            location.path = new_path;
            Ok(())
        })
    }

    /// Confirmed-intent removal: the location and its publication references leave in
    /// one mutation (docs/technical-design.md, "Unavailable and removed Discovery
    /// Locations"). Bundle deletion is later cleanup, not state's concern.
    pub fn remove_discovery_location(
        &self,
        id: DiscoveryLocationId,
    ) -> Result<MutationCommit, MutationError> {
        self.commit_mutation(|state| {
            let before = state.discovery_locations.len();
            state.discovery_locations.retain(|l| l.id != id);
            if state.discovery_locations.len() == before {
                return Err(MutationError::UnknownLocation);
            }
            state
                .publication_references
                .retain(|_, reference| reference.location != id);
            Ok(())
        })
    }

    /// The narrow capability `revisions` consumes in Phase 3.
    ///
    /// `installation` and `location` are trusted as a coherent pair: `ModInstallationId`
    /// derivation is one-way, so nothing here can verify that `installation` was
    /// actually derived from `location`. A mismatched pair would survive
    /// `remove_discovery_location`'s cascade as an unreachable reference. The state
    /// module deliberately does not own that guarantee — the caller (the application
    /// layer, mapping a scan result's installation and its owning location together)
    /// is the only place both halves of the pair are simultaneously in hand.
    pub fn set_publication_reference(
        &self,
        installation: ModInstallationId,
        location: DiscoveryLocationId,
        expected_prior: Option<&RevisionId>,
        next: RevisionId,
    ) -> Result<MutationCommit, PublicationError> {
        self.commit_publication(|state| {
            if !state.discovery_locations.iter().any(|l| l.id == location) {
                return Err(PublicationError::Mutation(MutationError::UnknownLocation));
            }
            let actual = state
                .publication_references
                .get(&installation)
                .map(|reference| reference.revision.clone());
            if actual.as_ref() != expected_prior {
                return Err(PublicationError::ExpectedMismatch { actual });
            }
            state.publication_references.insert(
                installation,
                PublicationReference {
                    location,
                    revision: next.clone(),
                },
            );
            Ok(())
        })
    }

    pub fn confirm_discard_unrecovered_references(&self) -> Result<MutationCommit, MutationError> {
        self.commit_mutation(|state| {
            state.unresolved_quarantine = None;
            Ok(())
        })
    }
}

/// The outcome of attempting one replacement while the store lock is held, before it has
/// been translated into either caller-facing error union.
enum ReplaceStep {
    Committed,
    CommittedDurabilityUncertain,
    StorageFailed(String),
    RecoveryRequired,
}

impl StateStore {
    /// Shared replacement mechanics for both mutation surfaces below: clone the state,
    /// let the caller apply its typed change, then commit through the replacement
    /// protocol and fold the result back into the lock. This is the plan's sanctioned
    /// simpler alternative to a generic multi-error-union `commit` — two thin public
    /// wrappers over one private replacement step, rather than a trait that maps each
    /// caller error type into a shared union.
    fn replace_locked(&self, inner: &mut MutexGuard<'_, Inner>, next: AppState) -> ReplaceStep {
        let next_bytes = encode(&next);
        let prior_bytes = inner.encoded.clone();
        let outcome = replace_state(
            inner.io_seam.as_mut(),
            self.state_path(),
            &next_bytes,
            Some(&prior_bytes),
        );
        match outcome {
            ReplaceOutcome::Committed => {
                inner.state = next;
                inner.encoded = next_bytes;
                ReplaceStep::Committed
            }
            ReplaceOutcome::CommittedDurabilityUncertain => {
                inner.state = next;
                inner.encoded = next_bytes;
                ReplaceStep::CommittedDurabilityUncertain
            }
            ReplaceOutcome::PriorRetained { detail } => ReplaceStep::StorageFailed(detail),
            ReplaceOutcome::RecoveryRequired { .. } => {
                inner.recovery_required = true;
                ReplaceStep::RecoveryRequired
            }
        }
    }

    fn commit_mutation(
        &self,
        apply: impl FnOnce(&mut AppState) -> Result<(), MutationError>,
    ) -> Result<MutationCommit, MutationError> {
        let mut inner = self.lock();
        if inner.recovery_required {
            return Err(MutationError::RecoveryRequired);
        }
        let mut next = inner.state.clone();
        apply(&mut next)?;
        match self.replace_locked(&mut inner, next) {
            ReplaceStep::Committed => Ok(MutationCommit::Committed),
            ReplaceStep::CommittedDurabilityUncertain => {
                Ok(MutationCommit::CommittedDurabilityUncertain)
            }
            ReplaceStep::StorageFailed(detail) => Err(MutationError::StorageFailed { detail }),
            ReplaceStep::RecoveryRequired => Err(MutationError::RecoveryRequired),
        }
    }

    fn commit_publication(
        &self,
        apply: impl FnOnce(&mut AppState) -> Result<(), PublicationError>,
    ) -> Result<MutationCommit, PublicationError> {
        let mut inner = self.lock();
        if inner.recovery_required {
            return Err(PublicationError::Mutation(MutationError::RecoveryRequired));
        }
        let mut next = inner.state.clone();
        apply(&mut next)?;
        match self.replace_locked(&mut inner, next) {
            ReplaceStep::Committed => Ok(MutationCommit::Committed),
            ReplaceStep::CommittedDurabilityUncertain => {
                Ok(MutationCommit::CommittedDurabilityUncertain)
            }
            ReplaceStep::StorageFailed(detail) => {
                Err(PublicationError::Mutation(MutationError::StorageFailed {
                    detail,
                }))
            }
            ReplaceStep::RecoveryRequired => {
                Err(PublicationError::Mutation(MutationError::RecoveryRequired))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::path::LogicalPath;
    use crate::discovery::identity::ModInstallationId;
    use crate::state::model::RevisionId;
    use crate::state::replace::{RealIo, ReplacementIo};
    use crate::state::store::{OpenOutcome, STATE_FILE, StateStore};
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn open(dir: &Path) -> StateStore {
        match StateStore::open(dir).unwrap() {
            OpenOutcome::Ready { store, .. } => store,
            OpenOutcome::BlockedNewerSchema { .. } => panic!("blocked"),
        }
    }

    fn installation(store: &StateStore, location: DiscoveryLocationId) -> ModInstallationId {
        let _ = store;
        ModInstallationId::derive(location, &LogicalPath::parse("ugc_1").unwrap())
    }

    #[test]
    fn add_rebind_and_remove_locations_persist_across_reopen() {
        let dir = TempDir::new().unwrap();
        let store = open(dir.path());
        let (id, _) = store
            .add_discovery_location(PathBuf::from("/tmp/workshop"))
            .unwrap();
        store
            .rebind_discovery_location(id, PathBuf::from("/tmp/moved-workshop"))
            .unwrap();
        drop(store);

        let store = open(dir.path());
        let locations = store.snapshot().discovery_locations;
        assert_eq!(locations.len(), 1);
        // A rebind preserves identity and changes only the path.
        assert_eq!(locations[0].id, id);
        assert_eq!(locations[0].path, PathBuf::from("/tmp/moved-workshop"));

        store.remove_discovery_location(id).unwrap();
        assert!(store.snapshot().discovery_locations.is_empty());
        assert!(matches!(
            store.rebind_discovery_location(id, PathBuf::from("/x")),
            Err(MutationError::UnknownLocation)
        ));
    }

    #[test]
    fn removing_a_location_cascades_only_its_publication_references() {
        let dir = TempDir::new().unwrap();
        let store = open(dir.path());
        let (kept, _) = store
            .add_discovery_location(PathBuf::from("/tmp/a"))
            .unwrap();
        let (removed, _) = store
            .add_discovery_location(PathBuf::from("/tmp/b"))
            .unwrap();
        let kept_install = installation(&store, kept);
        let removed_install = installation(&store, removed);
        store
            .set_publication_reference(kept_install, kept, None, RevisionId("r1".into()))
            .unwrap();
        store
            .set_publication_reference(removed_install, removed, None, RevisionId("r2".into()))
            .unwrap();

        store.remove_discovery_location(removed).unwrap();
        let refs = store.snapshot().publication_references;
        assert!(refs.contains_key(&kept_install));
        assert!(!refs.contains_key(&removed_install));
    }

    #[test]
    fn publication_reference_is_compare_and_swap() {
        let dir = TempDir::new().unwrap();
        let store = open(dir.path());
        let (location, _) = store
            .add_discovery_location(PathBuf::from("/tmp/a"))
            .unwrap();
        let install = installation(&store, location);

        // Expecting a prior on an empty slot fails and reports the actual value.
        let mismatch = store.set_publication_reference(
            install,
            location,
            Some(&RevisionId("ghost".into())),
            RevisionId("r1".into()),
        );
        assert!(matches!(
            mismatch,
            Err(PublicationError::ExpectedMismatch { actual: None })
        ));

        store
            .set_publication_reference(install, location, None, RevisionId("r1".into()))
            .unwrap();
        // Wrong expected prior fails and reports the actual.
        let mismatch = store.set_publication_reference(
            install,
            location,
            Some(&RevisionId("r0".into())),
            RevisionId("r2".into()),
        );
        match mismatch {
            Err(PublicationError::ExpectedMismatch { actual }) => {
                assert_eq!(actual, Some(RevisionId("r1".into())));
            }
            other => panic!("expected mismatch, got {other:?}"),
        }
        // Matching expected prior replaces.
        store
            .set_publication_reference(
                install,
                location,
                Some(&RevisionId("r1".into())),
                RevisionId("r2".into()),
            )
            .unwrap();
        assert_eq!(
            store.snapshot().publication_references[&install].revision,
            RevisionId("r2".into())
        );
    }

    #[test]
    fn publication_reference_requires_a_known_location() {
        let dir = TempDir::new().unwrap();
        let store = open(dir.path());
        let unknown = DiscoveryLocationId::generate();
        let install = installation(&store, unknown);
        assert!(matches!(
            store.set_publication_reference(install, unknown, None, RevisionId("r".into())),
            Err(PublicationError::Mutation(MutationError::UnknownLocation))
        ));
    }

    #[test]
    fn a_failed_mutation_changes_neither_memory_nor_disk() {
        struct AlwaysFail;
        impl ReplacementIo for AlwaysFail {
            fn write_temp(&mut self, _d: &Path, _b: &[u8]) -> io::Result<PathBuf> {
                Err(io::Error::other("injected failure"))
            }
            fn rename(&mut self, _f: &Path, _t: &Path) -> io::Result<()> {
                unreachable!("write_temp already failed")
            }
            fn sync_dir(&mut self, _d: &Path) -> io::Result<()> {
                unreachable!("write_temp already failed")
            }
        }
        let dir = TempDir::new().unwrap();
        // Seed a valid file with real I/O, then reopen with the failing seam.
        drop(open(dir.path()));
        let before = fs::read(dir.path().join(STATE_FILE)).unwrap();
        let store = match StateStore::open_with_io(dir.path(), Box::new(AlwaysFail)).unwrap() {
            OpenOutcome::Ready { store, .. } => store,
            OpenOutcome::BlockedNewerSchema { .. } => panic!("blocked"),
        };
        let result = store.add_discovery_location(PathBuf::from("/tmp/a"));
        assert!(matches!(result, Err(MutationError::StorageFailed { .. })));
        assert!(store.snapshot().discovery_locations.is_empty());
        assert_eq!(fs::read(dir.path().join(STATE_FILE)).unwrap(), before);
    }

    #[test]
    fn recovery_required_poisons_all_further_mutations() {
        struct CorruptingRename;
        impl ReplacementIo for CorruptingRename {
            fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
                RealIo.write_temp(dir, bytes)
            }
            fn rename(&mut self, _f: &Path, to: &Path) -> io::Result<()> {
                fs::write(to, b"neither prior nor next").unwrap();
                Err(io::Error::other("injected rename failure"))
            }
            fn sync_dir(&mut self, _d: &Path) -> io::Result<()> {
                Ok(())
            }
        }
        let dir = TempDir::new().unwrap();
        drop(open(dir.path()));
        let store = match StateStore::open_with_io(dir.path(), Box::new(CorruptingRename)).unwrap()
        {
            OpenOutcome::Ready { store, .. } => store,
            OpenOutcome::BlockedNewerSchema { .. } => panic!("blocked"),
        };
        assert!(matches!(
            store.add_discovery_location(PathBuf::from("/tmp/a")),
            Err(MutationError::RecoveryRequired)
        ));
        // Poisoned: even a mutation that would use fresh I/O is refused.
        assert!(matches!(
            store.confirm_discard_unrecovered_references(),
            Err(MutationError::RecoveryRequired)
        ));
    }

    #[test]
    fn confirm_discard_clears_the_persisted_notice() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(STATE_FILE), b"{garbage").unwrap();
        let store = open(dir.path());
        assert!(store.publication_recovery_unresolved().is_some());
        store.confirm_discard_unrecovered_references().unwrap();
        assert_eq!(store.publication_recovery_unresolved(), None);
        drop(store);
        assert_eq!(open(dir.path()).publication_recovery_unresolved(), None);
    }
}
