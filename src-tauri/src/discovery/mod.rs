//! Finds Stellaris and Mod Installations and reads only the metadata needed to populate
//! the Mod Library. Never a second fingerprint implementation
//! (docs/technical-design.md, "Source module"). Populated in Phase 1.

mod descriptor;
pub mod identity;
pub mod proposals;

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
    /// Identity only. NFC-normalized, so it is not necessarily the name on disk.
    pub relative_path: LogicalPath,
    /// The directory name exactly as enumeration read it: the only form that addresses
    /// the entry on a normalization-sensitive filesystem. Every filesystem access below
    /// this location joins this, never `relative_path`.
    pub raw_name: String,
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

/// Stands in for the name of an entry whose enumeration failed before a name was read.
const UNNAMED_ENTRY: &str = "(unreadable directory entry)";

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
    /// Enumeration or type inspection failed for this entry. Carried so classification
    /// can reject it visibly; an entry the scan could not look at is not an entry the
    /// scan may quietly claim is absent.
    Unreadable {
        label: String,
        reason: String,
    },
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
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                raw.push(RawEntry::Unreadable {
                    label: UNNAMED_ENTRY.to_owned(),
                    reason: format!("directory entry could not be read: {error}"),
                });
                continue;
            }
        };
        let name = entry.file_name();
        // `fs::metadata` follows symlinks, unlike `DirEntry::file_type`: a symlinked mod
        // directory is resolved and scanned under its lexical name
        // (docs/technical-design.md, "Installation identity").
        match fs::metadata(entry.path()) {
            Ok(metadata) if !metadata.is_dir() => continue,
            Ok(_) => match name.to_str() {
                Some(text) => raw.push(RawEntry::Directory(text.to_owned())),
                None => raw.push(RawEntry::InvalidUnicode(
                    name.to_string_lossy().into_owned(),
                )),
            },
            Err(error) => raw.push(RawEntry::Unreadable {
                label: name.to_string_lossy().into_owned(),
                reason: format!("entry could not be inspected: {error}"),
            }),
        }
    }
    let mut contents = classify_entries(location.id, raw);
    for installation in &mut contents.installations {
        // The raw name, never the logical path: `LogicalPath` normalizes to NFC, which
        // does not address an NFD directory name on a normalization-sensitive
        // filesystem.
        let descriptor_path = location
            .path
            .join(&installation.raw_name)
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
            RawEntry::Unreadable { label, reason } => rejected.push(RejectedEntry {
                raw_name: label,
                reason,
            }),
        }
    }
    let mut contents = LocationContents {
        rejected,
        ..LocationContents::default()
    };
    for (logical, mut raw_names) in by_logical {
        if raw_names.len() > 1 {
            contents
                .collisions
                .push(PathCollision { logical, raw_names });
        } else {
            let raw_name = raw_names
                .pop()
                .expect("a logical path has at least one name");
            contents.installations.push(ModInstallation {
                id: ModInstallationId::derive(location, &logical),
                location,
                relative_path: logical,
                raw_name,
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

    #[cfg(unix)]
    #[test]
    fn a_symlinked_directory_is_an_installation_under_its_lexical_name() {
        // Symlinks are resolved (followed) but keep their lexical root-relative path as
        // identity (docs/technical-design.md, "Installation identity"). A mod library
        // assembled out of symlinks is ordinary usage, not something to skip in silence.
        //
        // This pins only that the entry is discovered. The design's containment rule for
        // targets outside the canonical Discovery Location belongs to Mod Source
        // traversal (Phase 2) and no canonicalization exists here yet, so this test
        // deliberately asserts nothing about where the target lives.
        let elsewhere = TempDir::new().unwrap();
        let real = elsewhere.path().join("real_mod");
        fs::create_dir(&real).unwrap();
        fs::write(real.join("descriptor.mod"), "name=\"Linked Mod\"\n").unwrap();

        let dir = TempDir::new().unwrap();
        std::os::unix::fs::symlink(&real, dir.path().join("linked_mod")).unwrap();

        let scan = scan_location(&location(dir.path()));
        let LocationOutcome::Available(contents) = scan.outcome else {
            panic!("expected available");
        };
        assert_eq!(contents.installations.len(), 1);
        let installation = &contents.installations[0];
        assert_eq!(installation.relative_path.as_str(), "linked_mod");
        assert_eq!(
            installation.metadata.as_ref().unwrap().name.as_deref(),
            Some("Linked Mod")
        );
        assert!(contents.rejected.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn an_entry_whose_type_cannot_be_inspected_is_rejected_not_skipped() {
        let dir = TempDir::new().unwrap();
        std::os::unix::fs::symlink("nowhere", dir.path().join("dangling")).unwrap();
        fs::create_dir(dir.path().join("fine")).unwrap();

        let scan = scan_location(&location(dir.path()));
        let LocationOutcome::Available(contents) = scan.outcome else {
            panic!("expected available");
        };
        assert_eq!(contents.installations.len(), 1);
        assert_eq!(contents.rejected.len(), 1);
        assert_eq!(contents.rejected[0].raw_name, "dangling");
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
    fn an_installation_keeps_the_raw_name_the_filesystem_needs() {
        // Identity is NFC; the filesystem is not. On a normalization-sensitive
        // filesystem an NFD directory is only reachable under the bytes enumeration
        // read, so the raw name is carried rather than reconstructed from identity.
        let owner = DiscoveryLocationId::generate();
        let contents = classify_entries(
            owner,
            vec![RawEntry::Directory("te\u{301}ch_mod".to_owned())],
        );
        let installation = &contents.installations[0];
        assert_eq!(installation.raw_name, "te\u{301}ch_mod");
        assert_eq!(installation.relative_path.as_str(), "t\u{e9}ch_mod");
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
