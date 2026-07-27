//! Deterministic traversal of a Mod Source root into logical identities
//! (docs/technical-design.md, "Source module"; "Source snapshot consistency" step 1).
//!
//! Two stages with one authority each. [`enumerate`] touches the filesystem and produces
//! [`RawEntry`] values in whatever order the filesystem offered them, resolving links for
//! containment and cycle checks as it goes. [`classify_entries`] is pure: it applies the
//! enumeration policy, normalizes names into [`LogicalPath`]s, detects collisions, and
//! emits the inventory in canonical byte order.
//!
//! The seam exists because macOS cannot stage the interesting inputs. APFS is case- and
//! normalization-insensitive and refuses names that are not valid UTF-8, so a collision
//! or an invalid-Unicode entry can be tested only by handing raw entries to the pure
//! stage — the same reasoning as `discovery::classify_entries`.
//!
//! Nothing is dropped in silence. A name that cannot become a logical path, an entry
//! that cannot be resolved, a link leaving the root, and a link folding back into its own
//! containing path all become [`RejectedFile`]s; two raw entries normalizing to one
//! logical path become a [`FileCollision`] and neither wins. Files the policy simply does
//! not cover are not rejections: excluding a `.dds` is the policy working.

use crate::canonical::path::{LogicalPath, PathError};
use crate::source::policy::{self, FileFamily};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub logical: LogicalPath,
    pub family: FileFamily,
    pub raw_components: Vec<String>,
}

impl SourceFile {
    pub fn absolute_under(&self, root: &Path) -> PathBuf {
        let mut path = root.to_path_buf();
        for component in &self.raw_components {
            path.push(component);
        }
        path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCollision {
    pub logical: LogicalPath,
    pub raw_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedFile {
    pub raw_label: String,
    pub reason: RejectionReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectionReason {
    InvalidUnicode,
    InvalidPath(PathError),
    TraversalEscape { target: String },
    SymlinkCycle,
    Unreadable { detail: String },
}

impl fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUnicode => f.write_str("name is not valid Unicode"),
            Self::InvalidPath(error) => write!(f, "{error}"),
            Self::TraversalEscape { target } => {
                write!(f, "resolves outside the source root, to {target}")
            }
            Self::SymlinkCycle => f.write_str("link resolves into its own containing path"),
            Self::Unreadable { detail } => write!(f, "could not be inspected: {detail}"),
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SourceInventory {
    pub files: Vec<SourceFile>,
    pub collisions: Vec<FileCollision>,
    pub rejected: Vec<RejectedFile>,
}

#[derive(Debug)]
pub enum RawEntry {
    File { components: Vec<String> },
    InvalidUnicode { label: String },
    Escape { label: String, target: String },
    Cycle { label: String },
    Unreadable { label: String, detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootError {
    NotADirectory,
    Unreadable { detail: String },
}

impl fmt::Display for RootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotADirectory => f.write_str("source root is not a directory"),
            Self::Unreadable { detail } => write!(f, "source root could not be read: {detail}"),
        }
    }
}

impl std::error::Error for RootError {}

/// Every analysis-relevant file under `root`, in canonical logical-path order, plus the
/// collisions and rejections the walk observed.
pub fn enumerate(root: &Path) -> Result<SourceInventory, RootError> {
    let canonical_root = fs::canonicalize(root).map_err(|error| RootError::Unreadable {
        detail: error.to_string(),
    })?;
    if !canonical_root.is_dir() {
        return Err(RootError::NotADirectory);
    }
    let mut walk = Walk {
        canonical_root: canonical_root.clone(),
        raw: Vec::new(),
    };
    walk.visit(root, &canonical_root, &[], Descent::Root)
        .map_err(|error| RootError::Unreadable {
            detail: error.to_string(),
        })?;
    Ok(classify_entries(walk.raw))
}

/// Pure classification: policy, normalization, collision detection, and canonical order.
pub fn classify_entries(raw: Vec<RawEntry>) -> SourceInventory {
    let mut selected: BTreeMap<LogicalPath, (FileFamily, Vec<Vec<String>>)> = BTreeMap::new();
    let mut rejected = Vec::new();
    for entry in raw {
        match entry {
            RawEntry::File { components } => {
                let joined = components.join("/");
                match LogicalPath::parse(&joined) {
                    Ok(logical) => {
                        if let Some(family) = policy::family_for(&logical) {
                            selected
                                .entry(logical)
                                .or_insert((family, Vec::new()))
                                .1
                                .push(components);
                        }
                    }
                    Err(error) => rejected.push(RejectedFile {
                        raw_label: joined,
                        reason: RejectionReason::InvalidPath(error),
                    }),
                }
            }
            RawEntry::InvalidUnicode { label } => rejected.push(RejectedFile {
                raw_label: label,
                reason: RejectionReason::InvalidUnicode,
            }),
            RawEntry::Escape { label, target } => rejected.push(RejectedFile {
                raw_label: label,
                reason: RejectionReason::TraversalEscape { target },
            }),
            RawEntry::Cycle { label } => rejected.push(RejectedFile {
                raw_label: label,
                reason: RejectionReason::SymlinkCycle,
            }),
            RawEntry::Unreadable { label, detail } => rejected.push(RejectedFile {
                raw_label: label,
                reason: RejectionReason::Unreadable { detail },
            }),
        }
    }

    let mut files = Vec::new();
    let mut collisions = Vec::new();
    for (logical, (family, mut raw_names)) in selected {
        match raw_names.len() {
            1 => files.push(SourceFile {
                logical,
                family,
                raw_components: raw_names.remove(0),
            }),
            _ => {
                let mut raw_labels: Vec<String> = raw_names
                    .iter()
                    .map(|components| components.join("/"))
                    .collect();
                raw_labels.sort();
                collisions.push(FileCollision {
                    logical,
                    raw_labels,
                });
            }
        }
    }
    // Reports are results too: a rejection list whose order depended on walk order would
    // make two scans of one unchanged tree disagree.
    rejected.sort_by(|left, right| {
        left.raw_label
            .cmp(&right.raw_label)
            .then_with(|| left.reason.to_string().cmp(&right.reason.to_string()))
    });
    SourceInventory {
        files,
        collisions,
        rejected,
    }
}

/// Whether a directory being visited is the source root, where only the policy's
/// enumerated top-level directories are descended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Descent {
    Root,
    Below,
}

struct Walk {
    canonical_root: PathBuf,
    raw: Vec<RawEntry>,
}

impl Walk {
    /// `real` addresses the directory through the names on disk; `canonical` is its
    /// resolved location, carried for containment and cycle checks; `chain` is the raw
    /// root-relative component list that becomes logical identity.
    fn visit(
        &mut self,
        real: &Path,
        canonical: &Path,
        chain: &[String],
        descent: Descent,
    ) -> io::Result<()> {
        for entry in fs::read_dir(real)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.raw.push(RawEntry::Unreadable {
                        label: label_of(chain, "(unreadable directory entry)"),
                        detail: error.to_string(),
                    });
                    continue;
                }
            };
            let name = entry.file_name();
            let label = label_of(chain, &name.to_string_lossy());
            // No-follow: a link is resolved deliberately below, not incidentally.
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    self.raw.push(RawEntry::Unreadable {
                        label,
                        detail: error.to_string(),
                    });
                    continue;
                }
            };
            if file_type.is_symlink() {
                self.visit_link(&entry.path(), &name, canonical, chain, descent, label);
            } else if file_type.is_dir() {
                self.descend(&entry.path(), &canonical.join(&name), &name, chain, descent);
            } else if file_type.is_file() {
                self.record_file(&name, chain, label);
            }
            // Anything else (sockets, FIFOs, devices) is not source content.
        }
        Ok(())
    }

    fn visit_link(
        &mut self,
        real: &Path,
        name: &std::ffi::OsString,
        canonical: &Path,
        chain: &[String],
        descent: Descent,
        label: String,
    ) {
        // Resolution is what makes containment and cycles decidable; the lexical
        // root-relative path stays the identity either way
        // (docs/technical-design.md, "Installation identity").
        let target = match fs::canonicalize(real) {
            Ok(target) => target,
            Err(error) => {
                self.raw.push(RawEntry::Unreadable {
                    label,
                    detail: error.to_string(),
                });
                return;
            }
        };
        if !target.starts_with(&self.canonical_root) {
            self.raw.push(RawEntry::Escape {
                label,
                target: target.display().to_string(),
            });
            return;
        }
        if target.is_dir() {
            // A target that contains the directory we are standing in would repeat this
            // chain forever. A target elsewhere inside the root is legitimate: the same
            // physical file may appear under several logical paths.
            if canonical.starts_with(&target) {
                self.raw.push(RawEntry::Cycle { label });
                return;
            }
            self.descend(real, &target, name, chain, descent);
        } else if target.is_file() {
            self.record_file(name, chain, label);
        }
    }

    fn descend(
        &mut self,
        real: &Path,
        canonical: &Path,
        name: &std::ffi::OsString,
        chain: &[String],
        descent: Descent,
    ) {
        let Some(text) = name.to_str() else {
            // Not decodable, so no file beneath it could receive an identity. One
            // rejection naming the directory beats silently omitting its contents.
            self.raw.push(RawEntry::InvalidUnicode {
                label: label_of(chain, &name.to_string_lossy()),
            });
            return;
        };
        if descent == Descent::Root && !policy::enumerated_root_directories().contains(&text) {
            return;
        }
        let mut child_chain = chain.to_vec();
        child_chain.push(text.to_owned());
        if let Err(error) = self.visit(real, canonical, &child_chain, Descent::Below) {
            self.raw.push(RawEntry::Unreadable {
                label: child_chain.join("/"),
                detail: error.to_string(),
            });
        }
    }

    fn record_file(&mut self, name: &std::ffi::OsStr, chain: &[String], label: String) {
        if !policy::extension_may_be_enumerated(name.as_encoded_bytes()) {
            return;
        }
        match name.to_str() {
            Some(text) => {
                let mut components = chain.to_vec();
                components.push(text.to_owned());
                self.raw.push(RawEntry::File { components });
            }
            None => self.raw.push(RawEntry::InvalidUnicode { label }),
        }
    }
}

fn label_of(chain: &[String], name: &str) -> String {
    if chain.is_empty() {
        return name.to_owned();
    }
    format!("{}/{name}", chain.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(root: &Path, relative: &str, contents: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn logical_paths(inventory: &SourceInventory) -> Vec<&str> {
        inventory
            .files
            .iter()
            .map(|file| file.logical.as_str())
            .collect()
    }

    fn raw_file(path: &str) -> RawEntry {
        RawEntry::File {
            components: path.split('/').map(str::to_owned).collect(),
        }
    }

    /// A small mod-shaped tree: both families, nested directories, and content the
    /// policy excludes.
    fn staged_source(root: &Path) {
        write(root, "descriptor.mod", "name=\"Fixture\"\n");
        write(root, "common/technology/00_tech.txt", "tech_a = {}\n");
        write(root, "common/technology/01_tech.txt", "tech_b = {}\n");
        write(root, "events/fixture_events.txt", "namespace = fixture\n");
        write(root, "interface/fixture.gui", "guiTypes = {}\n");
        write(root, "gfx/interface/icons/icons.gfx", "spriteTypes = {}\n");
        write(root, "localisation/english/l_english.yml", "l_english:\n");
        // Excluded by policy.
        write(root, "gfx/models/ship.dds", "not really a dds");
        write(root, "sound/effects.txt", "sounds\n");
        write(root, "README.md", "# readme\n");
        write(root, "thumbnail.png", "png");
    }

    #[test]
    fn enumerates_both_families_in_canonical_path_order() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());

        let inventory = enumerate(dir.path()).unwrap();

        assert_eq!(
            logical_paths(&inventory),
            vec![
                "common/technology/00_tech.txt",
                "common/technology/01_tech.txt",
                "descriptor.mod",
                "events/fixture_events.txt",
                "gfx/interface/icons/icons.gfx",
                "interface/fixture.gui",
                "localisation/english/l_english.yml",
            ]
        );
        let localization = inventory
            .files
            .iter()
            .find(|file| file.family == FileFamily::Localization)
            .unwrap();
        assert_eq!(
            localization.logical.as_str(),
            "localisation/english/l_english.yml"
        );
        assert!(inventory.rejected.is_empty());
        assert!(inventory.collisions.is_empty());
    }

    #[test]
    fn identical_trees_under_different_roots_enumerate_identically() {
        // Logical identity is root-relative: the absolute source root never participates
        // (docs/technical-design.md, "Installation identity").
        let here = TempDir::new().unwrap();
        let there = TempDir::new().unwrap();
        staged_source(here.path());
        staged_source(there.path());

        let first = enumerate(here.path()).unwrap();
        let second = enumerate(there.path()).unwrap();

        assert_eq!(logical_paths(&first), logical_paths(&second));
        assert_ne!(here.path(), there.path());
    }

    #[test]
    fn enumeration_order_is_independent_of_directory_walk_order() {
        // The pure seam is where walk order can actually be varied: on any real
        // filesystem the walk order is whatever readdir returns.
        let forward = classify_entries(vec![
            raw_file("common/a.txt"),
            raw_file("events/b.txt"),
            raw_file("localisation/c.yml"),
        ]);
        let reversed = classify_entries(vec![
            raw_file("localisation/c.yml"),
            raw_file("events/b.txt"),
            raw_file("common/a.txt"),
        ]);
        assert_eq!(logical_paths(&forward), logical_paths(&reversed));
        assert_eq!(
            logical_paths(&forward),
            vec!["common/a.txt", "events/b.txt", "localisation/c.yml"]
        );
    }

    #[test]
    fn ordering_is_by_normalized_bytes_not_locale_or_case() {
        let inventory = classify_entries(vec![
            raw_file("common/zeta.txt"),
            raw_file("common/Alpha.txt"),
            raw_file("common/midway.txt"),
        ]);
        assert_eq!(
            logical_paths(&inventory),
            vec!["common/Alpha.txt", "common/midway.txt", "common/zeta.txt"]
        );
    }

    #[test]
    fn files_the_policy_excludes_are_absent_without_being_rejections() {
        let inventory = classify_entries(vec![
            raw_file("gfx/models/ship.dds"),
            raw_file("sound/effects.txt"),
            raw_file("common/kept.txt"),
        ]);
        assert_eq!(logical_paths(&inventory), vec!["common/kept.txt"]);
        assert!(inventory.rejected.is_empty());
    }

    // APFS is case- and normalization-insensitive and rejects invalid UTF-8 names
    // outright, so these classes cannot be staged on disk here. They are tested at the
    // pure seam, following the discovery::classify_entries precedent.
    #[test]
    fn names_that_are_not_valid_unicode_are_rejected_not_skipped() {
        let inventory = classify_entries(vec![
            RawEntry::InvalidUnicode {
                label: "common/bad\u{fffd}name.txt".to_owned(),
            },
            raw_file("common/fine.txt"),
        ]);
        assert_eq!(logical_paths(&inventory), vec!["common/fine.txt"]);
        assert_eq!(inventory.rejected.len(), 1);
        assert_eq!(
            inventory.rejected[0].reason,
            RejectionReason::InvalidUnicode
        );
    }

    #[test]
    fn entries_normalizing_to_one_logical_path_are_a_visible_collision() {
        let inventory = classify_entries(vec![
            raw_file("common/te\u{301}ch.txt"),
            raw_file("common/t\u{e9}ch.txt"),
            raw_file("common/unrelated.txt"),
        ]);
        // Neither collided entry wins: an arbitrary winner is silent data loss, and a
        // fingerprint over one of two candidate byte streams is not an identity.
        assert_eq!(logical_paths(&inventory), vec!["common/unrelated.txt"]);
        assert_eq!(inventory.collisions.len(), 1);
        assert_eq!(
            inventory.collisions[0].logical.as_str(),
            "common/t\u{e9}ch.txt"
        );
        assert_eq!(inventory.collisions[0].raw_labels.len(), 2);
    }

    #[test]
    fn names_that_cannot_be_a_logical_path_are_rejected() {
        let inventory = classify_entries(vec![
            raw_file("common/back\\slash.txt"),
            raw_file("common/fine.txt"),
        ]);
        assert_eq!(logical_paths(&inventory), vec!["common/fine.txt"]);
        assert_eq!(
            inventory.rejected[0].reason,
            RejectionReason::InvalidPath(PathError::BackslashComponent)
        );
    }

    #[test]
    fn rejection_reasons_render_for_a_report() {
        let reasons = [
            RejectionReason::InvalidUnicode,
            RejectionReason::InvalidPath(PathError::DotComponent),
            RejectionReason::TraversalEscape {
                target: "/elsewhere/mod".to_owned(),
            },
            RejectionReason::SymlinkCycle,
            RejectionReason::Unreadable {
                detail: "permission denied".to_owned(),
            },
        ];
        for reason in reasons {
            let rendered = reason.to_string();
            assert!(!rendered.is_empty());
            assert!(!rendered.contains("RejectionReason"), "{rendered}");
        }
    }

    #[test]
    fn an_unreadable_root_is_an_error_not_an_empty_inventory() {
        let dir = TempDir::new().unwrap();
        assert!(matches!(
            enumerate(&dir.path().join("never-created")),
            Err(RootError::Unreadable { .. })
        ));

        let file_root = dir.path().join("descriptor.mod");
        fs::write(&file_root, "name=\"x\"\n").unwrap();
        assert_eq!(enumerate(&file_root), Err(RootError::NotADirectory));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_resolving_outside_the_root_is_rejected() {
        // docs/technical-design.md, "Installation identity": targets outside the
        // canonical root are rejected. Both the file and the directory form, because a
        // linked directory would otherwise smuggle a whole foreign tree into identity.
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("foreign.txt"), "foreign\n").unwrap();
        fs::create_dir(outside.path().join("foreign_dir")).unwrap();
        fs::write(outside.path().join("foreign_dir/deep.txt"), "deep\n").unwrap();

        let dir = TempDir::new().unwrap();
        write(dir.path(), "common/real.txt", "real\n");
        std::os::unix::fs::symlink(
            outside.path().join("foreign.txt"),
            dir.path().join("common/escaped.txt"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("foreign_dir"),
            dir.path().join("common/escaped_dir"),
        )
        .unwrap();

        let inventory = enumerate(dir.path()).unwrap();

        assert_eq!(logical_paths(&inventory), vec!["common/real.txt"]);
        let mut labels: Vec<&str> = inventory
            .rejected
            .iter()
            .map(|entry| entry.raw_label.as_str())
            .collect();
        labels.sort_unstable();
        assert_eq!(labels, vec!["common/escaped.txt", "common/escaped_dir"]);
        assert!(
            inventory
                .rejected
                .iter()
                .all(|entry| matches!(entry.reason, RejectionReason::TraversalEscape { .. }))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_cycle_is_rejected_rather_than_walked_forever() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "common/technology/real.txt", "real\n");
        // Points at an ancestor of its own location: descending would repeat forever.
        std::os::unix::fs::symlink(
            dir.path().join("common"),
            dir.path().join("common/technology/loop"),
        )
        .unwrap();

        let inventory = enumerate(dir.path()).unwrap();

        assert_eq!(
            logical_paths(&inventory),
            vec!["common/technology/real.txt"]
        );
        assert_eq!(inventory.rejected.len(), 1);
        assert_eq!(inventory.rejected[0].reason, RejectionReason::SymlinkCycle);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_inside_the_root_is_followed_and_keeps_its_lexical_path() {
        // "Following a directory indirection may expose the same physical file under
        // multiple valid logical paths; those paths remain separate inputs because
        // Stellaris addresses their logical locations."
        let dir = TempDir::new().unwrap();
        write(dir.path(), "common/technology/real.txt", "real\n");
        std::os::unix::fs::symlink(
            dir.path().join("common/technology"),
            dir.path().join("common/mirror"),
        )
        .unwrap();

        let inventory = enumerate(dir.path()).unwrap();

        assert_eq!(
            logical_paths(&inventory),
            vec!["common/mirror/real.txt", "common/technology/real.txt"]
        );
        assert!(inventory.rejected.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn an_entry_that_cannot_be_resolved_is_rejected_not_skipped() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "common/real.txt", "real\n");
        std::os::unix::fs::symlink("nowhere.txt", dir.path().join("common/dangling.txt")).unwrap();

        let inventory = enumerate(dir.path()).unwrap();

        assert_eq!(logical_paths(&inventory), vec!["common/real.txt"]);
        assert_eq!(inventory.rejected.len(), 1);
        assert_eq!(inventory.rejected[0].raw_label, "common/dangling.txt");
        assert!(matches!(
            inventory.rejected[0].reason,
            RejectionReason::Unreadable { .. }
        ));
    }

    #[test]
    fn a_source_file_addresses_the_filesystem_through_its_raw_components() {
        // Identity is NFC; the filesystem is not. Reads join the bytes enumeration read.
        let inventory = classify_entries(vec![raw_file("common/te\u{301}ch.txt")]);
        let file = &inventory.files[0];
        assert_eq!(file.logical.as_str(), "common/t\u{e9}ch.txt");
        assert_eq!(
            file.absolute_under(Path::new("/roots/mod")),
            Path::new("/roots/mod/common/te\u{301}ch.txt")
        );
    }
}
