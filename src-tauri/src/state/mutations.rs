//! Typed mutations on the state store. The public surface is per-owner and narrow:
//! Discovery Location configuration for setup workflows, and the publication-reference
//! compare-and-swap that is the only mutation `revisions` receives
//! (docs/technical-design.md, "Mutable state storage"). Callers read immutable snapshots
//! and write through these typed mutations; the mutable document itself, and edits that
//! reach across concerns, are never exposed.

use super::model::{AppState, DiscoveryLocation, PublicationReference, RevisionId};
use super::replace::{ReplaceOutcome, replace_state};
use super::store::{Inner, StateStore, encode};
use crate::discovery::identity::{DiscoveryLocationId, ModInstallationId};
use crate::localization::LanguageTag;
use std::fmt;
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
    /// A Discovery Location path that is not valid UTF-8. The state document is JSON, so
    /// such a path has no representation in it; `discovery` rejects non-Unicode names on
    /// the same grounds rather than converting them lossily.
    PathNotUtf8 {
        path: PathBuf,
    },
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
        let path = storable_location_path(path)?;
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
        let new_path = storable_location_path(new_path)?;
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

    /// The publication compare-and-swap. This inherent method is the implementation;
    /// [`PublicationCapability::set_publication_reference`] is the grant, and — since this
    /// is visible only inside `state` — the only route any other module has to it. The
    /// caller obligation the two share is stated once, there.
    pub(super) fn set_publication_reference(
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

    /// Sets or clears the explicit desktop language override — the only durable part of the
    /// effective-language derivation (D-097).
    ///
    /// One method rather than a setter and a clearer, because `Option` already carries the
    /// distinction: `None` hands the language back to detection and is not a choice of English.
    ///
    /// No `storable_*` guard and no new [`MutationError`] variant, unlike
    /// [`add_discovery_location`](StateStore::add_discovery_location): a
    /// [`LanguageTag`](crate::localization::LanguageTag) is ASCII by construction, so there is
    /// no unrepresentable value for a guard to reject and `encode`'s totality argument is
    /// unchanged.
    pub fn set_language_override(
        &self,
        language: Option<LanguageTag>,
    ) -> Result<MutationCommit, MutationError> {
        self.commit_mutation(|state| {
            state.language_override = language;
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

/// The whole of `state` that publication is given: one compare-and-swap on one Mod
/// Installation's publication reference, and nothing else.
///
/// It delegates, so the decision it owns is what it *withholds*. A `&StateStore` handed to
/// `revisions` would also hand it [`StateStore::add_discovery_location`],
/// [`StateStore::rebind_discovery_location`], [`StateStore::remove_discovery_location`],
/// [`StateStore::confirm_discard_unrecovered_references`], and `snapshot` — the mutable
/// state document entire — leaving D-090's "`revisions` cannot modify unrelated settings
/// or access the mutable state representation" true only by discipline. This type makes it
/// true by construction: code holding one has no expressible path to the rest of the
/// store. The argument is the one `source::snapshot::LiveSource` makes for liveness — a
/// capability, not a flag.
///
/// It borrows rather than owns. The store's lifetime is the process and the composition
/// root owns it, so no shared ownership is needed to hand this to a collaborator.
pub struct PublicationCapability<'a> {
    store: &'a StateStore,
}

impl<'a> PublicationCapability<'a> {
    /// Constructed where a `&StateStore` is legitimately in hand: the composition root,
    /// or a test. Granting the capability is that caller's decision, which is why it is
    /// made explicitly here rather than derived inside `revisions`.
    pub fn new(store: &'a StateStore) -> Self {
        Self { store }
    }

    /// Atomically replaces `installation`'s publication reference, but only if the stored
    /// revision is `expected_prior` — `None` meaning "no reference is stored yet". A
    /// mismatch changes nothing and reports the actual stored revision.
    ///
    /// The caller owes one guarantee this layer cannot check: that `installation` and
    /// `location` are a coherent pair. `ModInstallationId` derivation is one-way, so
    /// nothing here can verify that `installation` came from `location`, and a mismatched
    /// pair would survive [`StateStore::remove_discovery_location`]'s cascade as an
    /// unreachable reference. That guarantee lives in the application layer, the only
    /// place a scan result's installation and its owning location are simultaneously in
    /// hand; passing this capability down does not move it.
    pub fn set_publication_reference(
        &self,
        installation: ModInstallationId,
        location: DiscoveryLocationId,
        expected_prior: Option<&RevisionId>,
        next: RevisionId,
    ) -> Result<MutationCommit, PublicationError> {
        self.store
            .set_publication_reference(installation, location, expected_prior, next)
    }
}

/// The single point where a caller-supplied path becomes one the state document can
/// hold. Every mutation that writes a Discovery Location path passes through here, so
/// encoding stays total and no unrepresentable value can reach the encoder — which runs
/// while the store lock is held, where a panic would leave the store unusable.
fn storable_location_path(path: PathBuf) -> Result<PathBuf, MutationError> {
    match path.to_str() {
        Some(_) => Ok(path),
        None => Err(MutationError::PathNotUtf8 { path }),
    }
}

impl fmt::Display for MutationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownLocation => f.write_str("no such Discovery Location"),
            Self::PathNotUtf8 { path } => write!(
                f,
                "Discovery Location path is not valid UTF-8: {}",
                path.display()
            ),
            Self::RecoveryRequired => {
                f.write_str("state recovery is required before further mutation")
            }
            Self::StorageFailed { detail } => {
                write!(f, "state was not written and remains unchanged: {detail}")
            }
        }
    }
}

impl std::error::Error for MutationError {}

impl fmt::Display for PublicationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedMismatch {
                actual: Some(actual),
            } => write!(
                f,
                "publication reference did not match the expected prior; stored revision is {}",
                actual.0
            ),
            Self::ExpectedMismatch { actual: None } => f.write_str(
                "publication reference did not match the expected prior; no revision is stored",
            ),
            Self::Mutation(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PublicationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ExpectedMismatch { .. } => None,
            Self::Mutation(error) => Some(error),
        }
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
    use crate::durability::DirectoryFlush;
    use crate::state::model::RevisionId;
    use crate::state::replace::{RealIo, ReplacementIo};
    use crate::state::store::{OpenOutcome, STATE_FILE, StateStore};
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
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
            fn sync_dir(&mut self, _d: &Path) -> io::Result<DirectoryFlush> {
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

        // The same guarantee for the language override, through the same seam: a mutation is
        // applied to memory only after replacement commits, so a failed one leaves the prior
        // preference rather than a half-applied one.
        assert!(matches!(
            store.set_language_override(Some(LanguageTag::parse("l_german").unwrap())),
            Err(MutationError::StorageFailed { .. })
        ));
        assert_eq!(store.language_override(), None);
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
            fn sync_dir(&mut self, _d: &Path) -> io::Result<DirectoryFlush> {
                Ok(DirectoryFlush::Flushed)
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
        assert!(matches!(
            store.set_language_override(None),
            Err(MutationError::RecoveryRequired)
        ));
    }

    #[test]
    fn a_language_override_persists_across_reopen_and_clearing_is_not_choosing_english() {
        let dir = TempDir::new().unwrap();
        let store = open(dir.path());
        let german = LanguageTag::parse("l_german").unwrap();
        store.set_language_override(Some(german.clone())).unwrap();
        assert_eq!(store.language_override(), Some(german.clone()));
        drop(store);

        let store = open(dir.path());
        assert_eq!(store.language_override(), Some(german));
        // Clearing hands the language back to detection. `None`, never Some(l_english): the
        // difference is whether a later game-language change moves the documentation language.
        store.set_language_override(None).unwrap();
        assert_eq!(store.language_override(), None);
        drop(store);
        assert_eq!(open(dir.path()).language_override(), None);
    }

    /// Fails the protocol steps the test switches on, so one store can be driven
    /// through an uncertain commit and then a pre-commit failure.
    #[derive(Clone, Copy, Default)]
    struct Script {
        fail_sync_dir: bool,
        fail_rename: bool,
    }

    struct Scripted {
        script: Arc<Mutex<Script>>,
        inner: RealIo,
    }

    impl ReplacementIo for Scripted {
        fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
            self.inner.write_temp(dir, bytes)
        }
        fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
            if self.script.lock().unwrap().fail_rename {
                return Err(io::Error::other("injected rename failure"));
            }
            self.inner.rename(from, to)
        }
        fn sync_dir(&mut self, dir: &Path) -> io::Result<DirectoryFlush> {
            if self.script.lock().unwrap().fail_sync_dir {
                return Err(io::Error::other("injected sync_dir failure"));
            }
            self.inner.sync_dir(dir)
        }
    }

    #[test]
    fn an_uncertain_commit_advances_memory_and_becomes_the_tracked_prior() {
        let dir = TempDir::new().unwrap();
        drop(open(dir.path()));
        let script = Arc::new(Mutex::new(Script::default()));
        let store = match StateStore::open_with_io(
            dir.path(),
            Box::new(Scripted {
                script: Arc::clone(&script),
                inner: RealIo,
            }),
        )
        .unwrap()
        {
            OpenOutcome::Ready { store, .. } => store,
            OpenOutcome::BlockedNewerSchema { .. } => panic!("blocked"),
        };

        script.lock().unwrap().fail_sync_dir = true;
        let uncertain = store.add_discovery_location(PathBuf::from("/tmp/uncertain"));
        assert!(matches!(
            uncertain,
            Ok((_, MutationCommit::CommittedDurabilityUncertain))
        ));
        // The replacement was visible, so memory advances with it.
        let locations = store.snapshot().discovery_locations;
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, PathBuf::from("/tmp/uncertain"));

        // `encoded` tracked the uncertain commit: a reported rename failure classifies
        // the untouched file as the prior the store holds. Had the uncertain arm left
        // `encoded` on the pre-mutation bytes, the file would match neither prior nor
        // next and this would be RecoveryRequired.
        {
            let mut script = script.lock().unwrap();
            script.fail_sync_dir = false;
            script.fail_rename = true;
        }
        assert!(matches!(
            store.add_discovery_location(PathBuf::from("/tmp/never")),
            Err(MutationError::StorageFailed { .. })
        ));

        // And the store is still usable: the uncertain outcome poisoned nothing.
        script.lock().unwrap().fail_rename = false;
        assert_eq!(
            store
                .add_discovery_location(PathBuf::from("/tmp/certain"))
                .unwrap()
                .1,
            MutationCommit::Committed
        );
        assert_eq!(store.snapshot().discovery_locations.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_location_path_is_a_typed_error_and_leaves_the_store_usable() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let dir = TempDir::new().unwrap();
        let store = open(dir.path());
        let invalid = PathBuf::from(OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff]));

        assert!(matches!(
            store.add_discovery_location(invalid.clone()),
            Err(MutationError::PathNotUtf8 { .. })
        ));
        // Nothing was written and the lock was never poisoned by a panic inside it.
        assert!(store.snapshot().discovery_locations.is_empty());
        let (id, _) = store
            .add_discovery_location(PathBuf::from("/tmp/fine"))
            .unwrap();
        assert!(matches!(
            store.rebind_discovery_location(id, invalid),
            Err(MutationError::PathNotUtf8 { .. })
        ));
        assert_eq!(
            store.snapshot().discovery_locations[0].path,
            PathBuf::from("/tmp/fine")
        );
    }

    #[test]
    fn a_panic_while_holding_the_lock_does_not_disable_the_store() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(open(dir.path()));
        let poisoner = Arc::clone(&store);
        let panicked = std::thread::spawn(move || {
            let _guard = poisoner.lock();
            panic!("deliberate panic while holding the state lock");
        })
        .join();
        assert!(panicked.is_err());

        // Poisoning is recovered from deliberately: `Inner` cannot be observed torn.
        store
            .add_discovery_location(PathBuf::from("/tmp/after-poison"))
            .unwrap();
        assert_eq!(store.snapshot().discovery_locations.len(), 1);
    }

    #[test]
    fn state_errors_display_and_implement_std_error() {
        // Application and transport call sites `?` and log these; both need Display,
        // and std::error::Error is what makes `?` conversion available (the same
        // rationale as IdParseError).
        fn assert_error<E: std::error::Error>(error: &E) -> String {
            error.to_string()
        }
        assert!(!assert_error(&MutationError::UnknownLocation).is_empty());
        assert!(
            assert_error(&MutationError::PathNotUtf8 {
                path: PathBuf::from("/tmp/x"),
            })
            .contains("/tmp/x")
        );
        assert!(assert_error(&MutationError::RecoveryRequired).contains("recovery"));
        assert!(
            assert_error(&MutationError::StorageFailed {
                detail: "disk on fire".to_owned(),
            })
            .contains("disk on fire")
        );
        assert!(
            assert_error(&PublicationError::ExpectedMismatch {
                actual: Some(RevisionId("r1".into())),
            })
            .contains("r1")
        );
        // A wrapped mutation failure keeps its own message and is reachable as a source.
        let wrapped = PublicationError::Mutation(MutationError::UnknownLocation);
        assert_eq!(
            assert_error(&wrapped),
            MutationError::UnknownLocation.to_string()
        );
        assert!(std::error::Error::source(&wrapped).is_some());
        assert!(
            !assert_error(&crate::state::store::OpenError {
                detail: "unreadable".to_owned(),
            })
            .is_empty()
        );
    }

    #[test]
    fn the_publication_capability_commits_and_refuses_exactly_as_the_store_does() {
        // D-090 gives `revisions` this capability and nothing else, so it is the whole
        // contract that module can observe. Every outcome the store mutation produces —
        // both refusals and the swap itself — has to survive the narrowing, or the
        // consumer would be programming against a different contract than the one the
        // store tests above pin down.
        let dir = TempDir::new().unwrap();
        let store = open(dir.path());
        let (location, _) = store
            .add_discovery_location(PathBuf::from("/tmp/a"))
            .unwrap();
        let install = installation(&store, location);
        let publication = PublicationCapability::new(&store);

        // An unknown location is refused, so the capability cannot plant a reference
        // that `remove_discovery_location`'s cascade could never reach.
        let unknown = DiscoveryLocationId::generate();
        assert!(matches!(
            publication.set_publication_reference(
                installation(&store, unknown),
                unknown,
                None,
                RevisionId("r".into()),
            ),
            Err(PublicationError::Mutation(MutationError::UnknownLocation))
        ));

        // Expecting a prior on an empty slot is refused and reports the actual value.
        assert!(matches!(
            publication.set_publication_reference(
                install,
                location,
                Some(&RevisionId("ghost".into())),
                RevisionId("r1".into()),
            ),
            Err(PublicationError::ExpectedMismatch { actual: None })
        ));

        assert_eq!(
            publication
                .set_publication_reference(install, location, None, RevisionId("r1".into()))
                .unwrap(),
            MutationCommit::Committed
        );

        // A wrong expected prior is refused and reports what is actually stored.
        match publication.set_publication_reference(
            install,
            location,
            Some(&RevisionId("r0".into())),
            RevisionId("r2".into()),
        ) {
            Err(PublicationError::ExpectedMismatch { actual }) => {
                assert_eq!(actual, Some(RevisionId("r1".into())));
            }
            other => panic!("expected mismatch, got {other:?}"),
        }

        // A matching expected prior replaces, and the replacement is what the store holds.
        publication
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
    fn the_publication_capability_passes_an_uncertain_commit_through() {
        // `CommittedDurabilityUncertain` is how a caller learns the pointer is visible
        // but its durability unconfirmed (docs/technical-design.md, "Mutable state
        // storage"). A capability that flattened it into `Committed` would let
        // `revisions` report a durability nothing observed.
        let dir = TempDir::new().unwrap();
        drop(open(dir.path()));
        let script = Arc::new(Mutex::new(Script::default()));
        let store = match StateStore::open_with_io(
            dir.path(),
            Box::new(Scripted {
                script: Arc::clone(&script),
                inner: RealIo,
            }),
        )
        .unwrap()
        {
            OpenOutcome::Ready { store, .. } => store,
            OpenOutcome::BlockedNewerSchema { .. } => panic!("blocked"),
        };
        let (location, _) = store
            .add_discovery_location(PathBuf::from("/tmp/a"))
            .unwrap();
        let install = installation(&store, location);

        script.lock().unwrap().fail_sync_dir = true;
        let commit = PublicationCapability::new(&store)
            .set_publication_reference(install, location, None, RevisionId("r1".into()))
            .unwrap();
        assert_eq!(commit, MutationCommit::CommittedDurabilityUncertain);
        // Uncertain still means visible: the reference is the one the store now holds.
        assert_eq!(
            store.snapshot().publication_references[&install].revision,
            RevisionId("r1".into())
        );
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
