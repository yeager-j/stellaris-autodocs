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
//! that cannot be resolved, a link leaving the root, and a link revisiting a directory
//! already on the current descent all become [`RejectedFile`]s; two raw entries
//! normalizing to one logical path become a [`FileCollision`] and neither wins. Files the
//! policy simply does not cover are not rejections: excluding a `.dds` is the policy
//! working.
//!
//! Traversal depth is bounded by the source tree, not by the operating system's link
//! limit: cycle detection consults the whole descent chain, so every link is followed at
//! most once per descent and the walk terminates on its own. That is what keeps the
//! enumerated file set a function of the source rather than of the host's `SYMLOOP_MAX`.
//!
//! Link resolution is exercised on Unix only. Windows junctions and other reparse points
//! are expected to behave the same way through `fs::canonicalize`, but that is untested
//! here and wants verification before a Windows release (Phase 12).

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
    /// The name the filesystem returned is not valid Unicode. Distinct from
    /// `InvalidPath(PathError::InvalidUnicode)`, which this walk cannot produce: bytes are
    /// decoded per entry, so an undecodable name never reaches `LogicalPath` parsing.
    InvalidUnicode,
    InvalidPath(PathError),
    TraversalEscape {
        target: String,
    },
    SymlinkCycle,
    /// `kind` is carried because callers act on the distinction: an entry that disappeared
    /// mid-scan means the source changed, while a permission failure will not succeed on
    /// retry, and the Mod Library must tell "gone" from "unreadable".
    Unreadable {
        kind: io::ErrorKind,
        detail: String,
    },
}

impl RejectionReason {
    /// The stable identity code for this reason, the only part of a rejection that enters
    /// a [`SourceFingerprint`](crate::source::SourceFingerprint).
    ///
    /// Durable: a code is quoted by every revision whose source had this gap, so a code is
    /// renamed only through the fingerprint change protocol, never in place. The match is
    /// exhaustive over both enums so a new reason — or a new
    /// [`PathError`] — cannot reach the digest without a deliberate code for it.
    ///
    /// The payloads are deliberately absent, each because it describes the host rather
    /// than the source:
    ///
    /// - `TraversalEscape::target` is a canonicalized absolute path, so it names where the
    ///   machine keeps things.
    /// - `Unreadable::detail` is an OS message: host- and locale-dependent.
    /// - `Unreadable::kind` is permission state, a property of the machine. `NotFound` and
    ///   `PermissionDenied` say the same thing to documentation — evidence absent — and
    ///   differ only in whether a retry could help. Folding the kind into durable identity
    ///   would give one source two revision identifiers on two machines.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidUnicode => "invalid-unicode",
            Self::InvalidPath(error) => match error {
                PathError::InvalidUnicode => "invalid-path:invalid-unicode",
                PathError::Empty => "invalid-path:empty",
                PathError::AbsolutePrefix => "invalid-path:absolute-prefix",
                PathError::EmptyComponent => "invalid-path:empty-component",
                PathError::DotComponent => "invalid-path:dot-component",
                PathError::BackslashComponent => "invalid-path:backslash-component",
                PathError::NulByte => "invalid-path:nul-byte",
            },
            Self::TraversalEscape { .. } => "traversal-escape",
            Self::SymlinkCycle => "symlink-cycle",
            Self::Unreadable { .. } => "unreadable",
        }
    }
}

impl fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUnicode => f.write_str("name is not valid Unicode"),
            Self::InvalidPath(error) => write!(f, "{error}"),
            Self::TraversalEscape { target } => {
                write!(f, "resolves outside the source root, to {target}")
            }
            Self::SymlinkCycle => f.write_str("link revisits a directory already being walked"),
            Self::Unreadable { detail, .. } => write!(f, "could not be inspected: {detail}"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceInventory {
    pub files: Vec<SourceFile>,
    pub collisions: Vec<FileCollision>,
    pub rejected: Vec<RejectedFile>,
}

impl SourceInventory {
    /// What this walk could not observe, in the shape the fingerprint and the Source
    /// Snapshot both consume.
    ///
    /// [`ObservationGaps::is_empty`] is the single authority on whether an observation
    /// covers its whole source; this type deliberately offers no second predicate that
    /// could drift from it.
    pub fn gaps(&self) -> ObservationGaps {
        ObservationGaps {
            collisions: self.collisions.clone(),
            rejected: self.rejected.clone(),
        }
    }
}

/// What a source observation could not see. Empty on both counts is the definition of
/// complete.
///
/// Homed here, beside the two types it is made of, rather than in `source::snapshot`
/// where the Source Snapshot consumes it: `fingerprint` and `enumerate` both need the
/// type, and `snapshot` already depends on both, so owning it there would make the
/// dependency mutual for no gain. `source::snapshot` re-exports the name.
///
/// Gaps are identity-bearing, not merely a report: they join the content set in a
/// [`SourceFingerprint`](crate::source::SourceFingerprint), so a source that stops being
/// broken is never mistaken for the source that was.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObservationGaps {
    pub collisions: Vec<FileCollision>,
    pub rejected: Vec<RejectedFile>,
}

impl ObservationGaps {
    pub fn is_empty(&self) -> bool {
        self.collisions.is_empty() && self.rejected.is_empty()
    }
}

#[derive(Debug)]
pub enum RawEntry {
    File {
        components: Vec<String>,
    },
    InvalidUnicode {
        label: String,
    },
    Escape {
        label: String,
        target: String,
    },
    Cycle {
        label: String,
    },
    Unreadable {
        label: String,
        kind: io::ErrorKind,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootError {
    NotADirectory,
    Unreadable { kind: io::ErrorKind, detail: String },
}

impl fmt::Display for RootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotADirectory => f.write_str("source root is not a directory"),
            Self::Unreadable { detail, .. } => {
                write!(f, "source root could not be read: {detail}")
            }
        }
    }
}

impl std::error::Error for RootError {}

/// Every analysis-relevant file under `root`, in canonical logical-path order, plus the
/// collisions and rejections the walk observed.
pub fn enumerate(root: &Path) -> Result<SourceInventory, RootError> {
    let canonical_root = fs::canonicalize(root).map_err(|error| RootError::Unreadable {
        kind: error.kind(),
        detail: error.to_string(),
    })?;
    if !canonical_root.is_dir() {
        return Err(RootError::NotADirectory);
    }
    let mut walk = Walk {
        canonical_root: canonical_root.clone(),
        descent_chain: vec![canonical_root],
        raw: Vec::new(),
    };
    walk.visit(root, &[], Descent::Root)
        .map_err(|error| RootError::Unreadable {
            kind: error.kind(),
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
            RawEntry::Unreadable {
                label,
                kind,
                detail,
            } => rejected.push(RejectedFile {
                raw_label: label,
                reason: RejectionReason::Unreadable { kind, detail },
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
    /// The canonical directories of the current descent, root first. Following a link
    /// makes this chain non-nested, which is exactly why cycle detection consults all of
    /// it rather than only the directory it is standing in.
    descent_chain: Vec<PathBuf>,
    raw: Vec<RawEntry>,
}

impl Walk {
    /// `real` addresses the directory through the names on disk; `chain` is the raw
    /// root-relative component list that becomes logical identity. The canonical location
    /// of this directory is the last entry of [`Walk::descent_chain`].
    fn visit(&mut self, real: &Path, chain: &[String], descent: Descent) -> io::Result<()> {
        for entry in fs::read_dir(real)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.raw.push(RawEntry::Unreadable {
                        label: label_of(chain, "(unreadable directory entry)"),
                        kind: error.kind(),
                        detail: error.to_string(),
                    });
                    continue;
                }
            };
            let name = entry.file_name();
            // No-follow: a link is resolved deliberately below, not incidentally.
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    self.raw.push(RawEntry::Unreadable {
                        label: label_of(chain, &name.to_string_lossy()),
                        kind: error.kind(),
                        detail: error.to_string(),
                    });
                    continue;
                }
            };
            if file_type.is_symlink() {
                self.visit_link(&entry.path(), &name, chain, descent);
            } else if file_type.is_dir() {
                let canonical = self.current_canonical().join(&name);
                self.descend(&entry.path(), canonical, &name, chain, descent);
            } else if file_type.is_file() {
                self.record_file(&name, chain);
            }
            // Anything else (sockets, FIFOs, devices) is not source content.
        }
        Ok(())
    }

    fn visit_link(
        &mut self,
        real: &Path,
        name: &std::ffi::OsString,
        chain: &[String],
        descent: Descent,
    ) {
        // Resolution is what makes containment and cycles decidable; the lexical
        // root-relative path stays the identity either way
        // (docs/technical-design.md, "Installation identity").
        let target = match fs::canonicalize(real) {
            Ok(target) => target,
            Err(error) => {
                self.raw.push(RawEntry::Unreadable {
                    label: label_of(chain, &name.to_string_lossy()),
                    kind: error.kind(),
                    detail: error.to_string(),
                });
                return;
            }
        };
        if !target.starts_with(&self.canonical_root) {
            self.raw.push(RawEntry::Escape {
                label: label_of(chain, &name.to_string_lossy()),
                target: target.display().to_string(),
            });
            return;
        }
        if target.is_dir() {
            // A cycle is a revisit of a directory already on this descent: the walk would
            // reach it again through itself. Self-ancestor loops (`a/link -> a`) and
            // mutual ones (`a/to_b -> b` with `b/to_a -> a`) are the same condition;
            // checking only the current directory catches the first and lets the second
            // run until the operating system's link limit, which differs per platform and
            // would make the file set depend on the host.
            //
            // The check is per-descent, not global, so a sibling branch may still expose
            // the same physical directory under a second logical path — those paths remain
            // separate inputs because Stellaris addresses logical locations.
            if self
                .descent_chain
                .iter()
                .any(|ancestor| ancestor.starts_with(&target))
            {
                self.raw.push(RawEntry::Cycle {
                    label: label_of(chain, &name.to_string_lossy()),
                });
                return;
            }
            self.descend(real, target, name, chain, descent);
        } else if target.is_file() {
            self.record_file(name, chain);
        }
    }

    fn descend(
        &mut self,
        real: &Path,
        canonical: PathBuf,
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
        if descent == Descent::Root && !policy::is_enumerated_root(text) {
            return;
        }
        let mut child_chain = chain.to_vec();
        child_chain.push(text.to_owned());
        self.descent_chain.push(canonical);
        let outcome = self.visit(real, &child_chain, Descent::Below);
        self.descent_chain.pop();
        if let Err(error) = outcome {
            self.raw.push(RawEntry::Unreadable {
                label: child_chain.join("/"),
                kind: error.kind(),
                detail: error.to_string(),
            });
        }
    }

    fn record_file(&mut self, name: &std::ffi::OsStr, chain: &[String]) {
        // Raw bytes are enough, and the label is built only for the few entries that need
        // one: NFC normalization never rewrites an ASCII extension, so a name the policy
        // could select cannot be normalized out of this prefilter.
        //
        // The top-level directory is what makes the question answerable in full here: an
        // undecodable name can only leave as a rejection, and rejecting a file the policy
        // excludes anyway would report the whole inventory incomplete over content that
        // was never wanted.
        let top_level = chain.first().map(String::as_str);
        if !policy::raw_name_may_be_enumerated(top_level, name.as_encoded_bytes()) {
            return;
        }
        match name.to_str() {
            Some(text) => {
                let mut components = chain.to_vec();
                components.push(text.to_owned());
                self.raw.push(RawEntry::File { components });
            }
            None => self.raw.push(RawEntry::InvalidUnicode {
                label: label_of(chain, &name.to_string_lossy()),
            }),
        }
    }

    fn current_canonical(&self) -> &Path {
        self.descent_chain.last().unwrap_or(&self.canonical_root)
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
        // Chosen so byte order and case-insensitive collation disagree: `Z` is 0x5a and
        // `a` is 0x61, so bytes put `Zeta` first while any case-folding or locale-aware
        // comparison puts `alpha` first. The `\u{c9}clair`/`eclair` pair pins the
        // non-ASCII half: NFC UTF-8 bytes place it after every ASCII name, where a locale
        // collation would file the two together. The previous fixture
        // ({Alpha, midway, zeta}) sorted identically under both rules and so could not
        // fail for the reason it was named after.
        let inventory = classify_entries(vec![
            raw_file("common/alpha.txt"),
            raw_file("common/\u{c9}clair.txt"),
            raw_file("common/Zeta.txt"),
            raw_file("common/eclair.txt"),
        ]);
        assert_eq!(
            logical_paths(&inventory),
            vec![
                "common/Zeta.txt",
                "common/alpha.txt",
                "common/eclair.txt",
                "common/\u{c9}clair.txt",
            ]
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
                kind: io::ErrorKind::PermissionDenied,
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
    fn an_inventory_with_collisions_or_rejections_has_gaps() {
        // Gaps are what an observation missed, and they are identity-bearing: the
        // fingerprint covers the surviving files *and* this set.
        assert!(
            classify_entries(vec![raw_file("common/a.txt")])
                .gaps()
                .is_empty()
        );
        let collided = classify_entries(vec![
            raw_file("common/te\u{301}ch.txt"),
            raw_file("common/t\u{e9}ch.txt"),
        ])
        .gaps();
        assert!(!collided.is_empty());
        assert_eq!(collided.collisions.len(), 1);
        assert!(collided.rejected.is_empty());

        let rejected = classify_entries(vec![RawEntry::InvalidUnicode {
            label: "common/bad\u{fffd}.txt".to_owned(),
        }])
        .gaps();
        assert!(!rejected.is_empty());
        assert_eq!(rejected.rejected.len(), 1);
    }

    #[test]
    fn pinned_rejection_reason_codes() {
        // Pinned durable identity: every stored revision whose source had a gap quotes
        // these strings through its fingerprint. Change protocol is the fingerprint's own
        // (source::fingerprint's module comment): a new domain version plus a bump of
        // `AnalysisVersionVector::source_enumeration`, never a re-spelling in place.
        //
        // A new `RejectionReason` or `PathError` variant fails to compile in `code`
        // rather than silently landing here, which is why the match is exhaustive over
        // both enums instead of using a wildcard.
        let codes: Vec<(RejectionReason, &str)> = vec![
            (RejectionReason::InvalidUnicode, "invalid-unicode"),
            (
                RejectionReason::InvalidPath(PathError::InvalidUnicode),
                "invalid-path:invalid-unicode",
            ),
            (
                RejectionReason::InvalidPath(PathError::Empty),
                "invalid-path:empty",
            ),
            (
                RejectionReason::InvalidPath(PathError::AbsolutePrefix),
                "invalid-path:absolute-prefix",
            ),
            (
                RejectionReason::InvalidPath(PathError::EmptyComponent),
                "invalid-path:empty-component",
            ),
            (
                RejectionReason::InvalidPath(PathError::DotComponent),
                "invalid-path:dot-component",
            ),
            (
                RejectionReason::InvalidPath(PathError::BackslashComponent),
                "invalid-path:backslash-component",
            ),
            (
                RejectionReason::InvalidPath(PathError::NulByte),
                "invalid-path:nul-byte",
            ),
            (
                RejectionReason::TraversalEscape {
                    target: "/elsewhere/mod".to_owned(),
                },
                "traversal-escape",
            ),
            (RejectionReason::SymlinkCycle, "symlink-cycle"),
            (
                RejectionReason::Unreadable {
                    kind: io::ErrorKind::NotFound,
                    detail: "no such file or directory".to_owned(),
                },
                "unreadable",
            ),
        ];
        for (reason, expected) in &codes {
            assert_eq!(reason.code(), *expected);
        }
        let distinct: std::collections::BTreeSet<&str> =
            codes.iter().map(|(reason, _)| reason.code()).collect();
        assert_eq!(distinct.len(), codes.len(), "reason codes must be distinct");
    }

    #[test]
    fn a_reason_code_ignores_the_host_dependent_payload() {
        // The reason a fingerprint may quote a code but never a payload: two machines
        // observing the same broken mod disagree about the absolute escape target, the OS
        // message, and often the error kind, but agree about what went wrong.
        assert_eq!(
            RejectionReason::TraversalEscape {
                target: "/Users/a/steam/foreign".to_owned(),
            }
            .code(),
            RejectionReason::TraversalEscape {
                target: "/home/b/.steam/elsewhere".to_owned(),
            }
            .code()
        );
        assert_eq!(
            RejectionReason::Unreadable {
                kind: io::ErrorKind::NotFound,
                detail: "No such file or directory (os error 2)".to_owned(),
            }
            .code(),
            RejectionReason::Unreadable {
                kind: io::ErrorKind::PermissionDenied,
                detail: "Permission denied (os error 13)".to_owned(),
            }
            .code()
        );
    }

    #[test]
    fn an_unreadable_root_is_an_error_not_an_empty_inventory() {
        let dir = TempDir::new().unwrap();
        // The kind is carried, not only the message: a root that is gone is a Discovery
        // Location that went away, which callers treat differently from one they may not
        // read.
        assert!(matches!(
            enumerate(&dir.path().join("never-created")),
            Err(RootError::Unreadable {
                kind: io::ErrorKind::NotFound,
                ..
            })
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
    fn mutually_linked_sibling_directories_are_a_cycle() {
        // Neither link points at its own ancestor, so a self-ancestor check misses this
        // shape entirely: the walk descends a -> b -> a -> b until the operating system
        // refuses at SYMLOOP_MAX. That limit differs per platform (32 here, 40 on Linux),
        // which would make the enumerated file set — and so the fingerprint — depend on
        // the host rather than on the source.
        let dir = TempDir::new().unwrap();
        write(dir.path(), "common/a/in_a.txt", "a\n");
        write(dir.path(), "common/b/in_b.txt", "b\n");
        std::os::unix::fs::symlink(
            dir.path().join("common/b"),
            dir.path().join("common/a/to_b"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            dir.path().join("common/a"),
            dir.path().join("common/b/to_a"),
        )
        .unwrap();

        let inventory = enumerate(dir.path()).unwrap();

        // Each link is followed once; the second hop would revisit a directory already on
        // that descent, which is the cycle.
        assert_eq!(
            logical_paths(&inventory),
            vec![
                "common/a/in_a.txt",
                "common/a/to_b/in_b.txt",
                "common/b/in_b.txt",
                "common/b/to_a/in_a.txt",
            ]
        );
        let cycles: Vec<&str> = inventory
            .rejected
            .iter()
            .filter(|entry| entry.reason == RejectionReason::SymlinkCycle)
            .map(|entry| entry.raw_label.as_str())
            .collect();
        assert_eq!(cycles, vec!["common/a/to_b/to_a", "common/b/to_a/to_b"]);
        assert_eq!(inventory.rejected.len(), 2);
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
        // A link to a name that does not exist is NotFound, not a permission problem: the
        // source changed, and a retry may well succeed.
        assert!(matches!(
            inventory.rejected[0].reason,
            RejectionReason::Unreadable {
                kind: io::ErrorKind::NotFound,
                ..
            }
        ));
        assert!(!inventory.gaps().is_empty());
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
