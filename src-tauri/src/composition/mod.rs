//! Composition root: constructs the concrete modules, process-lifetime shared state,
//! background execution resources, and the Tauri application. Framework types stay here
//! and in `transport`; application modules never import Tauri.
//!
//! The one decision this module owns that is not construction is **which
//! [`RevisionCandidateSource`] a build gets**, and it is expressed as a `cfg` rather than a
//! runtime choice so a release binary has no reachable path to the hand-authored one. See
//! [`candidate_source`].
//!
//! [`open_stores`] is deliberately free of Tauri, because everything that can go wrong at
//! startup happens inside it and a test cannot construct an `App`.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use tauri::Manager;

use crate::application::{DocumentationHost, RevisionCandidateSource};
use crate::revisions::RevisionStore;
use crate::state::{OpenOutcome, OpenReport, StateStore};

/// The revisions location inside the application-data directory.
///
/// A sibling of `state.json` rather than a child of anything: `revisions/staging` must sit on
/// the same filesystem as `revisions/bundles` for publication's commit to be a rename, and
/// keeping the whole tree under one application-owned root is what makes that true by layout.
const REVISIONS_DIR: &str = "revisions";

/// Everything the application needs from disk, opened.
pub struct Stores {
    pub state: StateStore,
    pub revisions: RevisionStore,
    /// Carried rather than consumed: [`OpenReport::Quarantined`] names a file a person may
    /// still restore, and Phase 10's recovery screen is what reads it. Startup must not
    /// resolve it — clearing the notice is an explicit user confirmation
    /// ([`confirm_discard_unrecovered_references`]), never a side effect of launching.
    ///
    /// [`confirm_discard_unrecovered_references`]: crate::state::StateStore::confirm_discard_unrecovered_references
    pub report: OpenReport,
}

/// Why the application cannot start.
///
/// **Every variant is a placeholder for a screen Phase 10 owns.** Refusing to start with a
/// message is the honest skeleton; the alternative — modelling "this host has no store" inside
/// every operation's expected-error union — would put a variant in each of them that only a
/// broken installation can produce, and would leave every later phase carrying it.
#[derive(Debug)]
pub enum StartupRefusal {
    ApplicationDataUnavailable {
        detail: String,
    },
    StateUnreadable {
        detail: String,
    },
    /// Possibly valid data owned by a newer application version: never overwritten, never
    /// migrated down, and the reason nothing at all is written on this path.
    StateFromANewerVersion {
        found: u32,
        supported: u32,
    },
    RevisionsUnavailable {
        detail: String,
    },
}

impl std::fmt::Display for StartupRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApplicationDataUnavailable { detail } => write!(
                f,
                "the application-data directory could not be prepared: {detail}"
            ),
            Self::StateUnreadable { detail } => {
                write!(f, "application state could not be opened: {detail}")
            }
            Self::StateFromANewerVersion { found, supported } => write!(
                f,
                "application state was written by a newer version (schema {found} is not \
                 {supported}) and was left untouched"
            ),
            Self::RevisionsUnavailable { detail } => {
                write!(f, "the revisions location could not be opened: {detail}")
            }
        }
    }
}

impl std::error::Error for StartupRefusal {}

/// Opens both stores under one application-data directory.
///
/// Ordering matters in one respect: nothing is written after the newer-schema refusal, because
/// a state document this build does not understand must survive having been started against.
pub fn open_stores(app_data: &Path) -> Result<Stores, StartupRefusal> {
    fs::create_dir_all(app_data).map_err(|error| StartupRefusal::ApplicationDataUnavailable {
        detail: error.to_string(),
    })?;

    let outcome = StateStore::open(app_data).map_err(|error| StartupRefusal::StateUnreadable {
        detail: error.detail,
    })?;
    let (state, report) = match outcome {
        OpenOutcome::Ready { store, report } => (store, report),
        OpenOutcome::BlockedNewerSchema { found, supported } => {
            return Err(StartupRefusal::StateFromANewerVersion { found, supported });
        }
    };

    let revisions = RevisionStore::open(&app_data.join(REVISIONS_DIR)).map_err(|error| {
        StartupRefusal::RevisionsUnavailable {
            detail: error.to_string(),
        }
    })?;

    Ok(Stores {
        state,
        revisions,
        report,
    })
}

/// The source a build gets, chosen at compile time.
///
/// A release build cannot construct
/// [`HandAuthoredCandidates`](crate::testsupport::HandAuthoredCandidates) because that type does
/// not exist in it, so "a shipped binary cannot publish a revision documenting nothing real" is
/// a property of the build rather than of this function's body.
#[cfg(not(feature = "test-support"))]
fn candidate_source(_state: &StateStore) -> Box<dyn RevisionCandidateSource> {
    Box::new(crate::application::NoAnalysisSource)
}

/// The `test-support` counterpart. It also seeds the Discovery Location and candidate the
/// skeleton thread needs, and reports the values a caller must pass to `build_documentation` —
/// without them a developer could enable the feature and still have nothing to aim the command
/// at, because an installation identifier is a digest nothing can guess.
#[cfg(feature = "test-support")]
fn candidate_source(state: &StateStore) -> Box<dyn RevisionCandidateSource> {
    let (target, source) = crate::testsupport::candidates::seed_skeleton_thread(state);
    eprintln!(
        "skeleton thread ready — build_documentation({{ locationId: \"{}\", modRoot: \"{}\" }}), \
         then get_entry_list({{ installation: \"{}\" }})",
        target.location(),
        target.mod_root().as_str(),
        target.installation(),
    );
    Box::new(source)
}

/// Scaffold command retained so the scaffold React page keeps working. The Phase 3
/// frontend bootstrap deletes it together with that page.
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! You've been greeted from Rust!")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // STE-19 registers `tauri-plugin-single-instance` HERE, before every other plugin: it
        // must observe a second launch before anything else touches the application-data
        // directory D-065 gives this process sole ownership of.
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // The path resolver needs an `AppHandle`, so construction happens here rather than
            // before the builder. The directory it names is derived from the configured bundle
            // identifier — the same identifier single-instance keys on, which is what makes
            // "one Desktop Host, one application-data directory" true rather than hoped.
            let app_data = app.path().app_data_dir()?;
            let stores = open_stores(&app_data)?;
            if let OpenReport::Quarantined { quarantined_to } = &stores.report {
                eprintln!(
                    "unreadable application state was moved aside; publication-reference \
                     recovery stays unresolved until it is restored or discarded: {}",
                    quarantined_to.display()
                );
            }
            let candidates = candidate_source(&stores.state);
            app.manage(Arc::new(DocumentationHost::new(
                stores.state,
                stores.revisions,
                candidates,
            )));
            Ok(())
        })
        // Written out in full because `generate_handler!` resolves each name's generated
        // macro alongside it, and those live in `transport::tauri` rather than here.
        .invoke_handler(tauri::generate_handler![
            greet,
            crate::transport::tauri::build_documentation,
            crate::transport::tauri::get_entry_list
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{CURRENT_SCHEMA, STATE_FILE};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn app_data() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("stellaris-docs");
        (dir, path)
    }

    #[test]
    fn the_revisions_root_is_opened_beside_the_state_document() {
        // Adjacency is a precondition of the publication protocol rather than a layout
        // preference: staging and bundles must share a filesystem for the commit to be a
        // rename, and putting the revisions root under the same application-owned directory is
        // what makes that hold without asking anybody where things live.
        let (_dir, path) = app_data();

        let stores = open_stores(&path).unwrap();

        assert!(matches!(stores.report, OpenReport::FirstLaunch));
        assert!(path.join(STATE_FILE).is_file());
        assert!(path.join(REVISIONS_DIR).join("bundles").is_dir());
        assert!(path.join(REVISIONS_DIR).join("staging").is_dir());
    }

    #[test]
    fn opening_twice_is_the_ordinary_case_and_reports_the_second_as_loaded() {
        let (_dir, path) = app_data();

        drop(open_stores(&path).unwrap());
        let second = open_stores(&path).unwrap();

        assert!(matches!(second.report, OpenReport::Loaded));
    }

    #[test]
    fn state_from_a_newer_version_refuses_startup_and_is_left_untouched() {
        let (_dir, path) = app_data();
        fs::create_dir_all(&path).unwrap();
        let newer = format!(r#"{{"schema":{}}}"#, CURRENT_SCHEMA + 1);
        fs::write(path.join(STATE_FILE), &newer).unwrap();

        let Err(refusal) = open_stores(&path) else {
            panic!("a newer schema must refuse startup");
        };

        assert!(matches!(
            refusal,
            StartupRefusal::StateFromANewerVersion { .. }
        ));
        assert_eq!(fs::read_to_string(path.join(STATE_FILE)).unwrap(), newer);
        // Nothing else was created either: a build that cannot read the state document has no
        // business establishing a revisions location beside it.
        assert!(!path.join(REVISIONS_DIR).exists());
    }

    #[test]
    fn startup_refusal_displays_and_implements_std_error() {
        let refusal = StartupRefusal::StateFromANewerVersion {
            found: 2,
            supported: 1,
        };
        assert!(refusal.to_string().contains("newer version"));
        let _: &dyn std::error::Error = &refusal;
    }
}
