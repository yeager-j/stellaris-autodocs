//! Finds Stellaris and Mod Installations and reads only the metadata needed to populate
//! the Mod Library. Never a second fingerprint implementation
//! (docs/technical-design.md, "Source module"). Populated in Phase 1.

mod descriptor;
pub mod identity;

pub use descriptor::DescriptorMetadata;

use crate::canonical::path::LogicalPath;
use descriptor::parse_descriptor;
use identity::{DiscoveryLocationId, ModInstallationId};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// A user-confirmed Discovery Location as the scan input. The application layer maps
/// stored `state::DiscoveryLocation` records into these; `discovery` never reads state.
#[derive(Debug, Clone)]
pub struct ConfiguredLocation {
    pub id: DiscoveryLocationId,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct LocationScan {
    pub location: DiscoveryLocationId,
    pub outcome: LocationOutcome,
}

#[derive(Debug)]
pub enum LocationOutcome {
    Available(LocationContents),
    /// A failed scan reports the location unavailable rather than empty; stored
    /// configuration and revisions are never dropped for it
    /// (docs/technical-design.md, "Unavailable and removed Discovery Locations").
    Unavailable {
        reason: String,
    },
}

#[derive(Debug, Default)]
pub struct LocationContents {
    /// Ordered by normalized logical-path bytes, never filesystem walk order.
    pub installations: Vec<ModInstallation>,
    pub collisions: Vec<PathCollision>,
    pub rejected: Vec<RejectedEntry>,
}

#[derive(Debug)]
pub struct ModInstallation {
    pub id: ModInstallationId,
    pub location: DiscoveryLocationId,
    pub relative_path: LogicalPath,
    /// Advisory descriptor metadata; absence is normal.
    pub metadata: Option<descriptor::DescriptorMetadata>,
}

/// Distinct raw entries normalizing to one logical path. Neither becomes an
/// installation: an arbitrary winner would be silent data loss.
#[derive(Debug)]
pub struct PathCollision {
    pub logical: LogicalPath,
    pub raw_names: Vec<String>,
}

#[derive(Debug)]
pub struct RejectedEntry {
    /// Lossy rendering for the human-facing report only; never identity.
    pub raw_name: String,
    pub reason: String,
}

/// A directory entry as enumeration observed it, before classification.
#[derive(Debug)]
pub enum RawEntry {
    Directory(String),
    /// The name could not be decoded as Unicode; carried as a lossy label.
    InvalidUnicode(String),
}

pub fn scan_location(location: &ConfiguredLocation) -> LocationScan {
    let entries = match fs::read_dir(&location.path) {
        Ok(entries) => entries,
        Err(error) => {
            return LocationScan {
                location: location.id,
                outcome: LocationOutcome::Unavailable {
                    reason: error.to_string(),
                },
            };
        }
    };
    let mut raw = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        match name.to_str() {
            Some(text) => raw.push(RawEntry::Directory(text.to_owned())),
            None => raw.push(RawEntry::InvalidUnicode(
                name.to_string_lossy().into_owned(),
            )),
        }
    }
    let mut contents = classify_entries(location.id, raw);
    for installation in &mut contents.installations {
        // Filesystem access uses the raw name, which for surviving installations is
        // byte-identical to the logical path (a normalization-changing name would have
        // classified differently only alongside a collision partner; the logical form
        // still addresses it on macOS's normalization-insensitive filesystems).
        let descriptor_path = location
            .path
            .join(installation.relative_path.as_str())
            .join("descriptor.mod");
        if let Ok(text) = fs::read_to_string(descriptor_path) {
            installation.metadata = Some(parse_descriptor(&text));
        }
    }
    LocationScan {
        location: location.id,
        outcome: LocationOutcome::Available(contents),
    }
}

/// Pure classification seam: collision and rejection rules are testable without a
/// filesystem, which macOS (case- and normalization-insensitive APFS) cannot stage.
pub fn classify_entries(
    location: DiscoveryLocationId,
    raw_entries: Vec<RawEntry>,
) -> LocationContents {
    let mut by_logical: BTreeMap<LogicalPath, Vec<String>> = BTreeMap::new();
    let mut rejected = Vec::new();
    for entry in raw_entries {
        match entry {
            RawEntry::Directory(name) => match LogicalPath::parse(&name) {
                Ok(logical) => by_logical.entry(logical).or_default().push(name),
                Err(error) => rejected.push(RejectedEntry {
                    raw_name: name,
                    reason: format!("invalid mod directory name: {error:?}"),
                }),
            },
            RawEntry::InvalidUnicode(label) => rejected.push(RejectedEntry {
                raw_name: label,
                reason: "directory name is not valid Unicode".to_owned(),
            }),
        }
    }
    let mut contents = LocationContents {
        rejected,
        ..LocationContents::default()
    };
    for (logical, raw_names) in by_logical {
        if raw_names.len() > 1 {
            contents
                .collisions
                .push(PathCollision { logical, raw_names });
        } else {
            contents.installations.push(ModInstallation {
                id: ModInstallationId::derive(location, &logical),
                location,
                relative_path: logical,
                metadata: None,
            });
        }
    }
    contents
}

#[cfg(test)]
mod tests {
    use super::*;
    use identity::DiscoveryLocationId;
    use std::fs;
    use tempfile::TempDir;

    fn location(path: &std::path::Path) -> ConfiguredLocation {
        ConfiguredLocation {
            id: DiscoveryLocationId::generate(),
            path: path.to_path_buf(),
        }
    }

    #[test]
    fn scans_child_directories_as_installations_with_descriptor_metadata() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("ugc_123")).unwrap();
        fs::write(
            dir.path().join("ugc_123/descriptor.mod"),
            "name=\"Some Mod\"\nsupported_version=\"4.4.*\"\n",
        )
        .unwrap();
        fs::create_dir(dir.path().join("bare_mod")).unwrap();
        // Root-level files are not installations.
        fs::write(dir.path().join("stray.mod"), "name=\"Stray\"\n").unwrap();

        let configured = location(dir.path());
        let scan = scan_location(&configured);
        let LocationOutcome::Available(contents) = scan.outcome else {
            panic!("expected available");
        };
        assert_eq!(contents.installations.len(), 2);
        let with_meta = contents
            .installations
            .iter()
            .find(|i| i.relative_path.as_str() == "ugc_123")
            .unwrap();
        assert_eq!(
            with_meta.metadata.as_ref().unwrap().name.as_deref(),
            Some("Some Mod")
        );
        assert_eq!(
            with_meta.id,
            identity::ModInstallationId::derive(
                configured.id,
                &crate::canonical::path::LogicalPath::parse("ugc_123").unwrap()
            )
        );
        let bare = contents
            .installations
            .iter()
            .find(|i| i.relative_path.as_str() == "bare_mod")
            .unwrap();
        assert!(bare.metadata.is_none());
        assert!(contents.collisions.is_empty());
    }

    #[test]
    fn installations_are_ordered_by_normalized_path_bytes() {
        let dir = TempDir::new().unwrap();
        for name in ["zeta", "Alpha", "midway"] {
            fs::create_dir(dir.path().join(name)).unwrap();
        }
        let scan = scan_location(&location(dir.path()));
        let LocationOutcome::Available(contents) = scan.outcome else {
            panic!("expected available");
        };
        let order: Vec<&str> = contents
            .installations
            .iter()
            .map(|i| i.relative_path.as_str())
            .collect();
        // Byte order: uppercase before lowercase; never filesystem walk order.
        assert_eq!(order, vec!["Alpha", "midway", "zeta"]);
    }

    #[test]
    fn an_unreadable_location_is_unavailable_not_empty() {
        let dir = TempDir::new().unwrap();
        let gone = dir.path().join("never-created");
        let scan = scan_location(&location(&gone));
        assert!(matches!(scan.outcome, LocationOutcome::Unavailable { .. }));
    }

    // Collisions cannot be created on APFS (case- and normalization-insensitive), so
    // the classification rule is tested at its pure seam with raw names.
    #[test]
    fn entries_normalizing_to_one_logical_path_are_a_visible_collision() {
        let owner = DiscoveryLocationId::generate();
        let contents = classify_entries(
            owner,
            vec![
                RawEntry::Directory("te\u{301}ch_mod".to_owned()),
                RawEntry::Directory("t\u{e9}ch_mod".to_owned()),
                RawEntry::Directory("unrelated".to_owned()),
            ],
        );
        assert_eq!(contents.installations.len(), 1);
        assert_eq!(
            contents.installations[0].relative_path.as_str(),
            "unrelated"
        );
        assert_eq!(contents.collisions.len(), 1);
        assert_eq!(contents.collisions[0].raw_names.len(), 2);
    }

    #[test]
    fn invalid_names_are_rejected_entries_not_silent_omissions() {
        let owner = DiscoveryLocationId::generate();
        let contents = classify_entries(
            owner,
            vec![
                RawEntry::InvalidUnicode("bad\u{fffd}name".to_owned()),
                RawEntry::Directory("fine".to_owned()),
            ],
        );
        assert_eq!(contents.installations.len(), 1);
        assert_eq!(contents.rejected.len(), 1);
    }
}
