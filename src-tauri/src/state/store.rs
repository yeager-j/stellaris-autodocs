//! The deep state store: one authoritative in-memory value, serialized mutations, and
//! schema-dispatched loading with quarantine recovery (docs/technical-design.md,
//! "Mutable state storage" and "State evolution and recovery").

use super::model::{AppState, CURRENT_SCHEMA};
use super::replace::{RealIo, ReplaceOutcome, ReplacementIo, replace_state};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const STATE_FILE: &str = "state.json";

pub struct StateStore {
    state_path: PathBuf,
    inner: Mutex<Inner>,
}

pub(super) struct Inner {
    pub(super) state: AppState,
    /// The exact authoritative bytes, kept so the next replacement can classify an
    /// ambiguous outcome against the true prior.
    pub(super) encoded: Vec<u8>,
    /// Set when reopen-validation failed; every further mutation is refused.
    pub(super) recovery_required: bool,
    pub(super) io_seam: Box<dyn ReplacementIo + Send>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OpenReport {
    FirstLaunch,
    Loaded,
    /// Unreadable state was moved aside and defaults persisted. Publication-reference
    /// recovery stays unresolved until the user restores the file or confirms discard.
    Quarantined {
        quarantined_to: PathBuf,
    },
}

pub enum OpenOutcome {
    Ready {
        store: StateStore,
        report: OpenReport,
    },
    /// Possibly valid data owned by a newer application version: never overwritten,
    /// never migrated down, no store constructed (docs/technical-design.md).
    BlockedNewerSchema { found: u32, supported: u32 },
}

#[derive(Debug)]
pub struct OpenError {
    pub detail: String,
}

#[derive(Deserialize)]
struct SchemaProbe {
    schema: u32,
}

impl StateStore {
    pub fn open(state_dir: &Path) -> Result<OpenOutcome, OpenError> {
        Self::open_with_io(state_dir, Box::new(RealIo))
    }

    pub fn open_with_io(
        state_dir: &Path,
        io_seam: Box<dyn ReplacementIo + Send>,
    ) -> Result<OpenOutcome, OpenError> {
        let state_path = state_dir.join(STATE_FILE);
        match fs::read(&state_path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let store = Self::persist_fresh(state_path, AppState::first_launch(), io_seam)?;
                Ok(OpenOutcome::Ready {
                    store,
                    report: OpenReport::FirstLaunch,
                })
            }
            Err(error) => Err(OpenError {
                detail: format!("reading state: {error}"),
            }),
            Ok(bytes) => Self::open_existing(state_path, bytes, io_seam),
        }
    }

    fn open_existing(
        state_path: PathBuf,
        bytes: Vec<u8>,
        io_seam: Box<dyn ReplacementIo + Send>,
    ) -> Result<OpenOutcome, OpenError> {
        if let Ok(probe) = serde_json::from_slice::<SchemaProbe>(&bytes) {
            if probe.schema > CURRENT_SCHEMA {
                return Ok(OpenOutcome::BlockedNewerSchema {
                    found: probe.schema,
                    supported: CURRENT_SCHEMA,
                });
            }
            // No supported older schemas exist yet; a future migration dispatches on
            // probe.schema here and re-persists through the normal replacement path.
            if probe.schema == CURRENT_SCHEMA
                && let Ok(state) = serde_json::from_slice::<AppState>(&bytes)
            {
                let store = StateStore {
                    state_path,
                    inner: Mutex::new(Inner {
                        state,
                        encoded: bytes,
                        recovery_required: false,
                        io_seam,
                    }),
                };
                return Ok(OpenOutcome::Ready {
                    store,
                    report: OpenReport::Loaded,
                });
            }
        }
        Self::quarantine(state_path, bytes, io_seam)
    }

    /// Move the unreadable document to a diagnostic name and persist defaults that
    /// carry the unresolved-recovery notice. The unreadable file is never overwritten
    /// in place.
    fn quarantine(
        state_path: PathBuf,
        bytes: Vec<u8>,
        io_seam: Box<dyn ReplacementIo + Send>,
    ) -> Result<OpenOutcome, OpenError> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let hash = Sha256::digest(&bytes);
        let short: String = hash[..4].iter().map(|b| format!("{b:02x}")).collect();
        let name = format!("{STATE_FILE}.quarantine-{timestamp}-{short}");
        let quarantined_to = state_path.with_file_name(&name);
        fs::rename(&state_path, &quarantined_to).map_err(|error| OpenError {
            detail: format!("quarantining state: {error}"),
        })?;

        let mut defaults = AppState::first_launch();
        defaults.unresolved_quarantine = Some(name);
        let store = Self::persist_fresh(state_path, defaults, io_seam)?;
        Ok(OpenOutcome::Ready {
            store,
            report: OpenReport::Quarantined { quarantined_to },
        })
    }

    fn persist_fresh(
        state_path: PathBuf,
        state: AppState,
        mut io_seam: Box<dyn ReplacementIo + Send>,
    ) -> Result<StateStore, OpenError> {
        let encoded = encode(&state);
        match replace_state(io_seam.as_mut(), &state_path, &encoded, None) {
            ReplaceOutcome::Committed | ReplaceOutcome::CommittedDurabilityUncertain => {
                Ok(StateStore {
                    state_path,
                    inner: Mutex::new(Inner {
                        state,
                        encoded,
                        recovery_required: false,
                        io_seam,
                    }),
                })
            }
            ReplaceOutcome::PriorRetained { detail }
            | ReplaceOutcome::RecoveryRequired { detail } => Err(OpenError {
                detail: format!("persisting initial state: {detail}"),
            }),
        }
    }

    pub fn snapshot(&self) -> AppState {
        self.inner
            .lock()
            .expect("state lock poisoned")
            .state
            .clone()
    }

    pub fn publication_recovery_unresolved(&self) -> Option<String> {
        self.inner
            .lock()
            .expect("state lock poisoned")
            .state
            .unresolved_quarantine
            .clone()
    }

    pub(super) fn state_path(&self) -> &Path {
        &self.state_path
    }

    pub(super) fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("state lock poisoned")
    }
}

/// Human-readable: the state file is small, user-adjacent, and read during recovery.
pub(super) fn encode(state: &AppState) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(state).expect("state encodes to JSON");
    bytes.push(b'\n');
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{AppState, CURRENT_SCHEMA};
    use std::fs;
    use tempfile::TempDir;

    fn ready(outcome: OpenOutcome) -> (StateStore, OpenReport) {
        match outcome {
            OpenOutcome::Ready { store, report } => (store, report),
            OpenOutcome::BlockedNewerSchema { .. } => panic!("unexpectedly blocked"),
        }
    }

    #[test]
    fn absent_state_begins_first_launch_and_persists_defaults() {
        let dir = TempDir::new().unwrap();
        let (store, report) = ready(StateStore::open(dir.path()).unwrap());
        assert_eq!(report, OpenReport::FirstLaunch);
        assert_eq!(store.snapshot(), AppState::first_launch());
        drop(store);

        let (reopened, report) = ready(StateStore::open(dir.path()).unwrap());
        assert_eq!(report, OpenReport::Loaded);
        assert_eq!(reopened.snapshot(), AppState::first_launch());
    }

    #[test]
    fn newer_schema_blocks_without_any_write() {
        let dir = TempDir::new().unwrap();
        let newer = br#"{"schema":99,"future_field":true}"#;
        fs::write(dir.path().join(STATE_FILE), newer).unwrap();
        match StateStore::open(dir.path()).unwrap() {
            OpenOutcome::BlockedNewerSchema { found, supported } => {
                assert_eq!(found, 99);
                assert_eq!(supported, CURRENT_SCHEMA);
            }
            OpenOutcome::Ready { .. } => panic!("newer schema must block"),
        }
        // The file is byte-identical and nothing else appeared beside it.
        assert_eq!(fs::read(dir.path().join(STATE_FILE)).unwrap(), newer);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn malformed_state_is_quarantined_with_original_bytes_preserved() {
        let dir = TempDir::new().unwrap();
        let garbage = b"{not json";
        fs::write(dir.path().join(STATE_FILE), garbage).unwrap();

        let (store, report) = ready(StateStore::open(dir.path()).unwrap());
        let OpenReport::Quarantined { quarantined_to } = &report else {
            panic!("expected quarantine, got {report:?}");
        };
        // Original bytes preserved under the diagnostic name, defaults persisted,
        // and the unresolved-recovery notice survives in the new document.
        assert_eq!(fs::read(quarantined_to).unwrap(), garbage);
        let name = quarantined_to.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("state.json.quarantine-"));
        assert_eq!(
            store.publication_recovery_unresolved(),
            Some(name.to_owned())
        );
        drop(store);

        let (reopened, report) = ready(StateStore::open(dir.path()).unwrap());
        assert_eq!(report, OpenReport::Loaded);
        assert!(reopened.publication_recovery_unresolved().is_some());
    }

    #[test]
    fn unknown_lower_schema_is_quarantined_not_guessed() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(STATE_FILE), br#"{"schema":0}"#).unwrap();
        let (_store, report) = ready(StateStore::open(dir.path()).unwrap());
        assert!(matches!(report, OpenReport::Quarantined { .. }));
    }
}
