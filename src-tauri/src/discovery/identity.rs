//! Stable identifiers for Discovery Locations and Mod Installations
//! (docs/technical-design.md, "Installation identity").
//!
//! A Discovery Location identifier is random at creation: the location's absolute path
//! is editable configuration rather than identity, so a rebind preserves it. A Mod
//! Installation identifier is the canonical digest of the location identifier plus the
//! normalized relative mod path: re-scanning is stable, editing a location's path
//! preserves derived identities, and moving a mod within a location creates a new one.
//! Absolute paths, titles, declared versions, and content fingerprints never enter.

use crate::canonical::encode::CanonicalDigest;
use crate::canonical::hex::{self, hex_string_serde};
use crate::canonical::path::LogicalPath;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DiscoveryLocationId([u8; 16]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdParseError;

impl fmt::Display for IdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid identifier: expected a lowercase hex string of the exact length")
    }
}

impl std::error::Error for IdParseError {}

impl DiscoveryLocationId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().into_bytes())
    }

    pub fn parse(text: &str) -> Result<Self, IdParseError> {
        hex::decode::<16>(text).map(Self).ok_or(IdParseError)
    }
}

impl fmt::Display for DiscoveryLocationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        hex::write(f, &self.0)
    }
}

/// Opaque and deterministic within the app's identifier scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModInstallationId([u8; 32]);

impl ModInstallationId {
    pub fn derive(location: DiscoveryLocationId, mod_root: &LogicalPath) -> Self {
        let mut digest = CanonicalDigest::new("stellaris-docs/mod-installation-id/v1");
        digest.text(&location.to_string()).text(mod_root.as_str());
        Self(digest.finish().0)
    }

    pub fn parse(text: &str) -> Result<Self, IdParseError> {
        hex::decode::<32>(text).map(Self).ok_or(IdParseError)
    }
}

impl fmt::Display for ModInstallationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        hex::write(f, &self.0)
    }
}

hex_string_serde!(DiscoveryLocationId, "expected 32 lowercase hex characters");
hex_string_serde!(ModInstallationId, "expected 64 lowercase hex characters");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::path::LogicalPath;
    use proptest::prelude::*;

    fn path(raw: &str) -> LogicalPath {
        LogicalPath::parse(raw).unwrap()
    }

    #[test]
    fn location_ids_are_32_hex_and_unique_and_round_trip() {
        let id = DiscoveryLocationId::generate();
        let rendered = id.to_string();
        assert_eq!(rendered.len(), 32);
        assert!(rendered.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(id, DiscoveryLocationId::generate());
        assert_eq!(DiscoveryLocationId::parse(&rendered), Ok(id));
        assert_eq!(DiscoveryLocationId::parse("zz"), Err(IdParseError));
        assert_eq!(
            DiscoveryLocationId::parse("000102030405060708090A0B0C0D0E0F"),
            Err(IdParseError)
        );
    }

    #[test]
    fn installation_id_is_deterministic_for_location_and_path() {
        let location = DiscoveryLocationId::generate();
        let first = ModInstallationId::derive(location, &path("ugc_123"));
        let second = ModInstallationId::derive(location, &path("ugc_123"));
        assert_eq!(first, second);
        let rendered = first.to_string();
        assert_eq!(rendered.len(), 64);
        assert_eq!(ModInstallationId::parse(&rendered), Ok(first));
    }

    #[test]
    fn different_location_or_path_changes_the_identity() {
        let here = DiscoveryLocationId::generate();
        let there = DiscoveryLocationId::generate();
        let base = ModInstallationId::derive(here, &path("acot"));
        assert_ne!(base, ModInstallationId::derive(there, &path("acot")));
        assert_ne!(base, ModInstallationId::derive(here, &path("acot2")));
    }

    #[test]
    fn case_only_path_variants_are_distinct_identities() {
        let location = DiscoveryLocationId::generate();
        assert_ne!(
            ModInstallationId::derive(location, &path("Giga")),
            ModInstallationId::derive(location, &path("giga"))
        );
    }

    #[test]
    fn nfc_equivalent_paths_share_one_identity() {
        // The same rename that changes bytes but not the logical path must not change
        // identity — this is what lets preferences and references survive re-scans.
        let location = DiscoveryLocationId::generate();
        assert_eq!(
            ModInstallationId::derive(location, &path("te\u{301}ch")),
            ModInstallationId::derive(location, &path("t\u{e9}ch"))
        );
    }

    #[test]
    fn id_parse_error_displays_and_implements_std_error() {
        // Task 5/9/10 call sites want to `?` or log a parse failure; both require
        // Display, and std::error::Error is what makes `?` conversion available.
        fn assert_error<E: std::error::Error>(_: &E) {}
        assert_error(&IdParseError);
        assert!(!IdParseError.to_string().is_empty());
    }

    #[test]
    fn corrupted_state_deserialize_error_names_the_offending_value() {
        let error = serde_json::from_str::<DiscoveryLocationId>("\"not-hex\"").unwrap_err();
        assert!(error.to_string().contains("not-hex"));
    }

    #[test]
    fn ids_serialize_as_hex_strings_and_round_trip_as_map_keys() {
        let location = DiscoveryLocationId::generate();
        let installation = ModInstallationId::derive(location, &path("ugc_1"));
        let json = serde_json::to_string(&location).unwrap();
        assert_eq!(json, format!("\"{location}\""));

        let map = std::collections::BTreeMap::from([(installation, 1u32)]);
        let encoded = serde_json::to_string(&map).unwrap();
        let decoded: std::collections::BTreeMap<ModInstallationId, u32> =
            serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, map);

        assert!(serde_json::from_str::<DiscoveryLocationId>("\"not-hex\"").is_err());
    }

    #[test]
    fn pinned_derivation_vector() {
        // Pinned golden vector: these values are durable state keys from Ticket B on.
        // If this test ever fails, the identity scheme changed. The protocol is a new
        // domain version (/v2) plus a state-schema migration for publication
        // references — never a re-pin in place.
        let location = DiscoveryLocationId::parse("000102030405060708090a0b0c0d0e0f").unwrap();
        assert_eq!(location.to_string(), "000102030405060708090a0b0c0d0e0f");
        assert_eq!(
            ModInstallationId::derive(location, &path("ugc_1")).to_string(),
            "1627894b569dc98eb4e88d015d25cd88772bee08e1da54baddc2e5fe4ec9f104"
        );
    }

    // Includes uppercase and combining marks (U+0301, U+0308) alongside the precomposed
    // é, matching path.rs's PATH_RE precedent, so the injectivity property exercises
    // case and NFC variants rather than only lowercase ASCII plus one precomposed form.
    const COMPONENT_RE: &str =
        "[a-zA-Z0-9_\u{e9}\u{301}\u{308}]{1,10}(/[a-zA-Z0-9_\u{e9}\u{301}\u{308}]{1,10}){0,2}";

    proptest! {
        #[test]
        fn derivation_is_injective_over_generated_inputs(
            a in COMPONENT_RE,
            b in COMPONENT_RE,
        ) {
            // Same location: distinct logical paths must yield distinct identities.
            // Digest framing (Phase 0) is what rules out concatenation collisions.
            let location = DiscoveryLocationId::parse(
                "00000000000000000000000000000001",
            ).unwrap();
            let left = ModInstallationId::derive(location, &path(&a));
            let right = ModInstallationId::derive(location, &path(&b));
            prop_assert_eq!(left == right, path(&a) == path(&b));
        }

        #[test]
        fn parse_display_round_trips(seed in any::<[u8; 16]>()) {
            let original_hex = seed.iter().map(|b| format!("{b:02x}")).collect::<String>();
            let parsed = DiscoveryLocationId::parse(&original_hex).unwrap();
            prop_assert_eq!(parsed.to_string(), original_hex);
            prop_assert_eq!(DiscoveryLocationId::parse(&parsed.to_string()), Ok(parsed));
        }
    }
}
