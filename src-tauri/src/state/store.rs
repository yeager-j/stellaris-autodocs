//! The deep state store: one authoritative in-memory value, serialized mutations, and
//! schema-dispatched loading with quarantine recovery (docs/technical-design.md,
//! "Mutable state storage" and "State evolution and recovery").

use super::model::{AppState, CURRENT_SCHEMA};
use super::replace::{RealIo, ReplaceOutcome, ReplacementIo, replace_state, sweep_stale_temps};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt;
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
        Self::open_seamed(state_dir, Box::new(RealIo))
    }

    /// Replacement-I/O seam for tests only. Behind the `test-support` feature so it
    /// cannot reach a shipped binary (docs/decision-log.md, D-107).
    #[cfg(any(test, feature = "test-support"))]
    pub fn open_with_io(
        state_dir: &Path,
        io_seam: Box<dyn ReplacementIo + Send>,
    ) -> Result<OpenOutcome, OpenError> {
        Self::open_seamed(state_dir, io_seam)
    }

    fn open_seamed(
        state_dir: &Path,
        io_seam: Box<dyn ReplacementIo + Send>,
    ) -> Result<OpenOutcome, OpenError> {
        let state_path = state_dir.join(STATE_FILE);
        match fs::read(&state_path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                sweep_stale_temps(state_dir);
                let mut defaults = AppState::first_launch();
                let mut report = OpenReport::FirstLaunch;
                // An absent state file beside a leftover quarantine is an interrupted
                // recovery — the quarantine rename committed but persisting defaults
                // did not — never a clean first launch. The unresolved notice must
                // survive, or cleanup could discard artifacts whose publication
                // references are still recoverable from the quarantined bytes.
                if let Some(name) = newest_quarantine(state_dir) {
                    report = OpenReport::Quarantined {
                        quarantined_to: state_dir.join(&name),
                    };
                    defaults.unresolved_quarantine = Some(name);
                }
                let store = Self::persist_fresh(state_path, defaults, io_seam)?;
                Ok(OpenOutcome::Ready { store, report })
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
        let probe = serde_json::from_slice::<SchemaProbe>(&bytes);
        if let Ok(probe) = &probe
            && probe.schema > CURRENT_SCHEMA
        {
            // A blocked open touches nothing: no writes, and no sweep — abandoned
            // temporaries here may belong to the newer application version.
            return Ok(OpenOutcome::BlockedNewerSchema {
                found: probe.schema,
                supported: CURRENT_SCHEMA,
            });
        }
        // From here this application owns the directory; earlier runs' abandoned
        // temporaries are safe to remove.
        if let Some(dir) = state_path.parent() {
            sweep_stale_temps(dir);
        }
        if let Ok(probe) = probe {
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
        self.lock().state.clone()
    }

    pub fn publication_recovery_unresolved(&self) -> Option<String> {
        self.lock().state.unresolved_quarantine.clone()
    }

    pub(super) fn state_path(&self) -> &Path {
        &self.state_path
    }

    /// Recovers from poisoning rather than propagating it, deliberately: `Inner` cannot
    /// be observed torn. `replace_locked` assigns `state` and `encoded` adjacently with
    /// no fallible step between them, and `recovery_required` is set alone; every other
    /// holder of this guard only reads. A panic elsewhere in the process therefore
    /// leaves a consistent value here, and refusing the lock for the process lifetime
    /// would turn an unrelated panic into permanent, unrecoverable loss of state
    /// mutation.
    pub(super) fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Human-readable: the state file is small, user-adjacent, and read during recovery.
///
/// Serialization is total for `AppState`: its only non-string-shaped field is a
/// `DiscoveryLocation` path, and non-UTF-8 paths are rejected at the mutation boundary
/// (`mutations::storable_location_path`), which is what keeps this `expect` unreachable.
pub(super) fn encode(state: &AppState) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(state).expect("state encodes to JSON");
    bytes.push(b'\n');
    bytes
}

/// The lexically greatest quarantine file name, which is the newest while the
/// timestamp prefix keeps its digit count (until the year 33658).
fn newest_quarantine(dir: &Path) -> Option<String> {
    let entries = fs::read_dir(dir).ok()?;
    let prefix = format!("{STATE_FILE}.quarantine-");
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(&prefix))
        .max()
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "state could not be opened: {}", self.detail)
    }
}

impl std::error::Error for OpenError {}

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
    fn absent_state_beside_a_leftover_quarantine_resumes_recovery() {
        // The quarantine rename can commit while persisting defaults fails (ENOSPC);
        // the next startup must not present a clean first launch, or cleanup could
        // discard artifacts whose references are still recoverable.
        let dir = TempDir::new().unwrap();
        let name = "state.json.quarantine-1753000000-deadbeef";
        fs::write(dir.path().join(name), b"{unrecovered garbage").unwrap();

        let (store, report) = ready(StateStore::open(dir.path()).unwrap());
        assert!(matches!(report, OpenReport::Quarantined { .. }));
        assert_eq!(
            store.publication_recovery_unresolved().as_deref(),
            Some(name)
        );
        // The quarantined bytes are untouched and the notice survives reopen.
        assert!(dir.path().join(name).exists());
        drop(store);
        let (reopened, _) = ready(StateStore::open(dir.path()).unwrap());
        assert_eq!(
            reopened.publication_recovery_unresolved().as_deref(),
            Some(name)
        );
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
        // A blocked open touches nothing — not even another version's abandoned
        // temporary, because a newer schema means this directory is not ours.
        let foreign_temp = dir.path().join(".state-foreign.tmp");
        fs::write(&foreign_temp, b"newer app's staging bytes").unwrap();
        match StateStore::open(dir.path()).unwrap() {
            OpenOutcome::BlockedNewerSchema { found, supported } => {
                assert_eq!(found, 99);
                assert_eq!(supported, CURRENT_SCHEMA);
            }
            OpenOutcome::Ready { .. } => panic!("newer schema must block"),
        }
        // The file is byte-identical and nothing appeared or vanished beside it.
        assert_eq!(fs::read(dir.path().join(STATE_FILE)).unwrap(), newer);
        assert!(foreign_temp.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
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
    fn opening_sweeps_temporaries_abandoned_by_an_earlier_run() {
        // A crash or a failed write between create and rename leaves a `.state-*.tmp`
        // no later replacement will ever name again; open is the one moment no
        // replacement of this store is in flight, so it is where they are collected.
        let dir = TempDir::new().unwrap();
        drop(ready(StateStore::open(dir.path()).unwrap()).0);
        let stale = dir.path().join(".state-abandoned.tmp");
        fs::write(&stale, b"partial").unwrap();

        let (store, report) = ready(StateStore::open(dir.path()).unwrap());
        assert_eq!(report, OpenReport::Loaded);
        assert!(!stale.exists());
        // The sweep is selective, not a directory wipe.
        assert_eq!(store.snapshot(), AppState::first_launch());
        assert!(dir.path().join(STATE_FILE).exists());
    }

    #[test]
    fn unknown_lower_schema_is_quarantined_not_guessed() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(STATE_FILE), br#"{"schema":0}"#).unwrap();
        let (_store, report) = ready(StateStore::open(dir.path()).unwrap());
        assert!(matches!(report, OpenReport::Quarantined { .. }));
    }
}
