//! First-run Discovery Location proposals and Stellaris install detection
//! (docs/technical-design.md: setup "detects proposed Discovery Locations but allows
//! user correction"). Everything here is a proposal for the user to confirm or edit —
//! nothing is a hard-coded product path. macOS Steam defaults only; other platforms
//! arrive with their release work (AGENTS.md, "Stellaris environment reference").

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub struct ProposedLocations {
    pub workshop_mods: Option<PathBuf>,
    pub local_mods: Option<PathBuf>,
}

/// The Stellaris user-data root: the game's own configuration and the local mod directory both
/// live under it.
///
/// Named once because on macOS it sits inside the user's Documents folder, where the operating
/// system may refuse access — so it is the root two different features have to reason about
/// denial for (docs/technical-design.md, "Localization module" and "Unavailable and removed
/// Discovery Locations").
pub fn stellaris_user_data_root(home: &Path) -> PathBuf {
    home.join("Documents/Paradox Interactive/Stellaris")
}

/// The game's own settings file, whose top-level `language` assignment is the detected Stellaris
/// language. Named beside the root rather than inside `localization` so that "which file answers
/// this" has the same single home [`read_installed_build`] argues for `launcher-settings.json`;
/// `localization::detect_language` takes the path, which is what keeps it from depending on this
/// module.
pub fn stellaris_settings_file(home: &Path) -> PathBuf {
    stellaris_user_data_root(home).join("settings.txt")
}

pub fn propose_locations(home: &Path) -> ProposedLocations {
    let workshop = home.join("Library/Application Support/Steam/steamapps/workshop/content/281990");
    let local = stellaris_user_data_root(home).join("mod");
    ProposedLocations {
        workshop_mods: workshop.is_dir().then_some(workshop),
        local_mods: local.is_dir().then_some(local),
    }
}

/// `launcher-settings.json` is authoritative for the installed build; the game.log
/// banner reflects only the last run and is deliberately not read here (AGENTS.md,
/// "Version pinning").
#[derive(Debug)]
pub struct StellarisInstall {
    pub root: PathBuf,
    pub version: Option<String>,
    pub raw_version: Option<String>,
    pub mods_compatibility_version: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct LauncherSettings {
    version: Option<String>,
    raw_version: Option<String>,
    mods_compatibility_version: Option<String>,
}

pub fn detect_stellaris_install(home: &Path) -> Option<StellarisInstall> {
    let root = home.join("Library/Application Support/Steam/steamapps/common/Stellaris");
    root.is_dir().then(|| read_installed_build(root))
}

/// Reads the installed build from a root the caller already has.
///
/// Split out of [`detect_stellaris_install`], which proposes the macOS Steam location, so
/// that a caller pointed at an explicitly configured root — the local-corpus conformance
/// run, whose roots are environment-overridable — learns the build the same way rather than
/// parsing `launcher-settings.json` a second time. Which file answers "which build is
/// installed" has one home.
///
/// An unreadable or invalid settings file still yields an install with `None` versions: the
/// directory is the evidence that Stellaris is there, and the versions are what it says
/// about itself.
pub fn read_installed_build(root: PathBuf) -> StellarisInstall {
    let settings: LauncherSettings = fs::read(root.join("launcher-settings.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    StellarisInstall {
        root,
        version: settings.version,
        raw_version: settings.raw_version,
        mods_compatibility_version: settings.mods_compatibility_version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn proposes_only_locations_that_exist() {
        let home = TempDir::new().unwrap();
        let workshop = home
            .path()
            .join("Library/Application Support/Steam/steamapps/workshop/content/281990");
        fs::create_dir_all(&workshop).unwrap();

        let proposals = propose_locations(home.path());
        assert_eq!(proposals.workshop_mods, Some(workshop));
        // The local mod directory was never created, so it is not proposed.
        assert_eq!(proposals.local_mods, None);
    }

    #[test]
    fn the_local_mod_directory_and_the_settings_file_share_one_root() {
        // The single-home claim as an assertion: a second spelling of the Documents path would
        // make one of these two disagree with the other.
        let home = TempDir::new().unwrap();
        let root = stellaris_user_data_root(home.path());
        assert_eq!(stellaris_settings_file(home.path()).parent(), Some(&*root));

        fs::create_dir_all(root.join("mod")).unwrap();
        assert_eq!(
            propose_locations(home.path()).local_mods,
            Some(root.join("mod"))
        );
    }

    #[test]
    fn reads_launcher_settings_for_the_installed_build() {
        let home = TempDir::new().unwrap();
        let install = home
            .path()
            .join("Library/Application Support/Steam/steamapps/common/Stellaris");
        fs::create_dir_all(&install).unwrap();
        fs::write(
            install.join("launcher-settings.json"),
            r#"{"version":"Pegasus v4.4.6","rawVersion":"4.4.6","modsCompatibilityVersion":"4.4","exePath":"stellaris.app/Contents/MacOS/stellaris"}"#,
        )
        .unwrap();

        let found = detect_stellaris_install(home.path()).unwrap();
        assert_eq!(found.root, install);
        assert_eq!(found.version.as_deref(), Some("Pegasus v4.4.6"));
        assert_eq!(found.raw_version.as_deref(), Some("4.4.6"));
        assert_eq!(found.mods_compatibility_version.as_deref(), Some("4.4"));
    }

    #[test]
    fn an_install_without_readable_settings_is_still_detected() {
        let home = TempDir::new().unwrap();
        let install = home
            .path()
            .join("Library/Application Support/Steam/steamapps/common/Stellaris");
        fs::create_dir_all(&install).unwrap();
        let found = detect_stellaris_install(home.path()).unwrap();
        assert_eq!(found.root, install);
        assert_eq!(found.version, None);
    }

    #[test]
    fn no_install_directory_means_no_detection() {
        let home = TempDir::new().unwrap();
        assert!(detect_stellaris_install(home.path()).is_none());
    }
}
