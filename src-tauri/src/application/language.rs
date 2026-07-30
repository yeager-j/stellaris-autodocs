//! The effective desktop language, composed from the three authorities that own its parts:
//! `state` for the durable override, `discovery` for where Stellaris keeps its configuration,
//! and `localization` for what the configuration says and how the three sources rank.

use crate::discovery::proposals::stellaris_settings_file;
use crate::localization::{EffectiveLanguage, derive_effective_language, detect_language};
use crate::state::StateStore;
use std::path::Path;

/// Derives the effective desktop language from the stored override and a **fresh** read of
/// Stellaris configuration.
///
/// It holds nothing and caches nothing, and that is the requirement rather than an omission:
/// "The detected Stellaris language is refreshed from current game configuration during startup
/// and explicit Refresh rather than copied into the mutable-state authority"
/// (docs/technical-design.md, "Localization module"). A value that remembered the detected
/// language would be the second authority that sentence forbids, so there is no such value —
/// which also makes the refresh points a consequence of calling this rather than a cache
/// invalidation somebody has to get right.
///
/// `home` is a parameter rather than a `$HOME` read, matching
/// [`propose_locations`](crate::discovery::proposals::propose_locations) and
/// [`detect_stellaris_install`](crate::discovery::proposals::detect_stellaris_install): it is
/// what lets the whole derivation be exercised over a temporary directory.
pub fn effective_language(state: &StateStore, home: &Path) -> EffectiveLanguage {
    let settings = stellaris_settings_file(home);
    derive_effective_language(state.language_override(), detect_language(&settings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localization::{DetectedGameLanguage, LanguageSource, LanguageTag};
    use crate::state::{OpenOutcome, STATE_FILE};
    use std::fs;
    use tempfile::TempDir;

    fn store(dir: &Path) -> StateStore {
        match StateStore::open(dir).unwrap() {
            OpenOutcome::Ready { store, .. } => store,
            OpenOutcome::BlockedNewerSchema { .. } => panic!("blocked"),
        }
    }

    fn write_game_language(home: &Path, tag: &str) {
        let settings = stellaris_settings_file(home);
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        fs::write(&settings, format!("language=\"{tag}\"\r\n")).unwrap();
    }

    fn tag(text: &str) -> LanguageTag {
        LanguageTag::parse(text).unwrap()
    }

    #[test]
    fn a_game_language_change_changes_the_effective_language_with_no_state_mutation() {
        // The design's guarantee: "Without an override, a later game-language change therefore
        // changes the effective documentation language automatically." The absence of a state
        // mutation is half the claim, so the state file's bytes are asserted too.
        let app_data = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let store = store(app_data.path());
        write_game_language(home.path(), "l_french");

        let before_bytes = fs::read(app_data.path().join(STATE_FILE)).unwrap();
        let before = store.snapshot();
        let first = effective_language(&store, home.path());
        assert_eq!(first.language(), &tag("l_french"));
        assert_eq!(first.source(), LanguageSource::DetectedGameLanguage);

        write_game_language(home.path(), "l_german");
        let second = effective_language(&store, home.path());
        assert_eq!(second.language(), &tag("l_german"));
        assert_eq!(second.source(), LanguageSource::DetectedGameLanguage);

        assert_eq!(store.snapshot(), before);
        assert_eq!(
            fs::read(app_data.path().join(STATE_FILE)).unwrap(),
            before_bytes
        );
    }

    #[test]
    fn an_explicit_override_survives_a_game_language_change() {
        let app_data = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let store = store(app_data.path());
        store.set_language_override(Some(tag("l_polish"))).unwrap();

        write_game_language(home.path(), "l_french");
        let first = effective_language(&store, home.path());
        write_game_language(home.path(), "l_german");
        let second = effective_language(&store, home.path());

        for effective in [&first, &second] {
            assert_eq!(effective.language(), &tag("l_polish"));
            assert_eq!(effective.source(), LanguageSource::ExplicitOverride);
            assert_eq!(effective.configuration_access_denied(), None);
        }
    }

    #[test]
    fn a_home_without_stellaris_configuration_falls_back_to_english() {
        let app_data = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let effective = effective_language(&store(app_data.path()), home.path());
        assert_eq!(effective.language(), &LanguageTag::english());
        assert_eq!(effective.source(), LanguageSource::EnglishFallback);
        assert_eq!(effective.detected(), &DetectedGameLanguage::SettingsAbsent);
    }

    #[test]
    fn the_settings_file_read_is_the_one_discovery_names() {
        // Negative control for the single-home claim: this test writes the path by hand rather
        // than through `stellaris_settings_file`, so the two spellings are proven equal rather
        // than assumed. A second spelling of the Documents path makes this the failing test.
        let app_data = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let by_hand = home
            .path()
            .join("Documents/Paradox Interactive/Stellaris/settings.txt");
        fs::create_dir_all(by_hand.parent().unwrap()).unwrap();
        fs::write(&by_hand, b"language=\"l_korean\"\r\n").unwrap();

        let effective = effective_language(&store(app_data.path()), home.path());
        assert_eq!(effective.language(), &tag("l_korean"));
    }

    /// The real machine rather than a staged temporary directory: the only check that the byte
    /// scanner reads the settings file Stellaris actually writes, CRLF, the `graphics` block,
    /// the `soundgroup` decoy and all. Ignored like the corpus runs, and it **fails rather than
    /// skips** when the file is absent (src-tauri/AGENTS.md, "Building and running").
    #[test]
    #[ignore = "requires an installed Stellaris that has been run at least once"]
    fn detects_the_installed_games_language() {
        let home = std::env::var("HOME").expect("HOME is set");
        let settings = stellaris_settings_file(Path::new(&home));
        let detected = detect_language(&settings);
        println!("{}: {detected:?}", settings.display());
        assert!(
            matches!(detected, DetectedGameLanguage::Detected(_)),
            "{} did not yield a language: {detected:?}",
            settings.display()
        );
    }
}
