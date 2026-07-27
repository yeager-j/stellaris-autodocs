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
use crate::canonical::path::LogicalPath;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DiscoveryLocationId([u8; 16]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdParseError;

impl DiscoveryLocationId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().into_bytes())
    }

    pub fn parse(text: &str) -> Result<Self, IdParseError> {
        decode_hex::<16>(text).map(Self).ok_or(IdParseError)
    }
}

impl fmt::Display for DiscoveryLocationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(f, &self.0)
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
        decode_hex::<32>(text).map(Self).ok_or(IdParseError)
    }
}

impl fmt::Display for ModInstallationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(f, &self.0)
    }
}

fn write_hex(f: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    for byte in bytes {
        write!(f, "{byte:02x}")?;
    }
    Ok(())
}

fn decode_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let bytes = text.as_bytes();
    if bytes.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (slot, pair) in out.iter_mut().zip(bytes.chunks_exact(2)) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        *slot = ((hi << 4) | lo) as u8;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::path::LogicalPath;

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
}
