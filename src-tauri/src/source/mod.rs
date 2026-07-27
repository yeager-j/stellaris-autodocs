//! Sole owner of complete Mod Source traversal and content identity: deterministic
//! enumeration, logical-path normalization and escape rejection, hashing, fingerprints,
//! build-lifetime Source Snapshots, and final live-source verification
//! (docs/technical-design.md, "Source module"). Populated in Phase 2.
//!
//! [`scan`] is the module's whole Phase 2A surface: enumerate, hash, fingerprint. It
//! reads the live filesystem twice over a build's life — once here and once for the
//! pre-publication verification — which the design accepts as correctness-first
//! (docs/technical-design.md, "Source snapshot consistency"). Build-lifetime Source
//! Snapshots and that final verification arrive in later Phase 2 tickets on top of these
//! primitives.

pub mod enumerate;
pub mod fingerprint;
pub mod policy;

pub use enumerate::{
    FileCollision, RejectedFile, RejectionReason, RootError, SourceFile, SourceInventory,
};
pub use fingerprint::{ContentHash, DuplicateLogicalPath, HashParseError, SourceFingerprint};
pub use policy::FileFamily;

use crate::canonical::path::LogicalPath;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

/// One observation of a Mod Source: what was enumerated, what it contained, and the one
/// value that names both.
///
/// **The fingerprint covers `inventory.files` and nothing else.** A source with a
/// normalization collision or a rejected entry still scans to `Ok`, because the report of
/// what could not be enumerated is a result worth returning — but that fingerprint
/// describes a strict subset of the source. Check
/// [`SourceInventory::is_complete`](enumerate::SourceInventory::is_complete) before
/// treating a fingerprint as revision identity: publishing an incomplete observation
/// would pin a Documentation Revision to source the build never saw, and a later scan
/// that resolves the collision would look like an unrelated content change.
#[derive(Debug)]
pub struct SourceScan {
    pub inventory: SourceInventory,
    /// Keyed by the logical paths of `inventory.files`, one hash per enumerated file.
    pub contents: BTreeMap<LogicalPath, ContentHash>,
    /// Derived from `contents`, hence from the surviving files only. See the type comment.
    pub fingerprint: SourceFingerprint,
}

#[derive(Debug)]
pub enum ScanError {
    Root(RootError),
    /// An enumerated file could not be read. The whole scan fails: a fingerprint computed
    /// over a subset of the source would silently claim to describe the source.
    ///
    /// `kind` is carried because the caller's next move depends on it: `NotFound` means
    /// the source changed under the scan, which is an expected build outcome worth
    /// retrying, while `PermissionDenied` will not resolve itself.
    Read {
        logical: LogicalPath,
        kind: std::io::ErrorKind,
        detail: String,
    },
    /// Unreachable through [`scan`], whose inventory reports collisions instead of
    /// duplicating a path. Typed rather than asserted, because nothing panics across a
    /// module boundary (src/error.rs).
    Duplicate(DuplicateLogicalPath),
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root(error) => write!(f, "{error}"),
            Self::Read {
                logical, detail, ..
            } => {
                write!(f, "source file {logical} could not be read: {detail}")
            }
            Self::Duplicate(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ScanError {}

impl From<RootError> for ScanError {
    fn from(error: RootError) -> Self {
        Self::Root(error)
    }
}

/// Enumerates `root`, hashes the exact bytes of every enumerated file, and derives the
/// source fingerprint from that content.
///
/// Files are read through the raw names enumeration observed, never through the
/// NFC-normalized identity: on a normalization-sensitive filesystem the identity does not
/// address the file.
pub fn scan(root: &Path) -> Result<SourceScan, ScanError> {
    let inventory = enumerate::enumerate(root)?;
    let mut contents = BTreeMap::new();
    for file in &inventory.files {
        let content =
            ContentHash::of_file(&file.absolute_under(root)).map_err(|error| ScanError::Read {
                logical: file.logical.clone(),
                kind: error.kind(),
                detail: error.to_string(),
            })?;
        contents.insert(file.logical.clone(), content);
    }
    let fingerprint = SourceFingerprint::of(
        contents
            .iter()
            .map(|(logical, content)| (logical.clone(), *content)),
    )
    .map_err(ScanError::Duplicate)?;
    Ok(SourceScan {
        inventory,
        contents,
        fingerprint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(root: &Path, relative: &str, contents: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn staged_source(root: &Path) {
        write(root, "descriptor.mod", b"name=\"Fixture\"\n");
        write(root, "common/technology/00_tech.txt", b"tech_a = {}\n");
        write(root, "common/technology/01_tech.txt", b"tech_b = {}\n");
        write(root, "localisation/english/l_english.yml", b"l_english:\n");
        write(root, "gfx/models/ship.dds", b"binary-ish");
    }

    fn fingerprint_of(root: &Path) -> SourceFingerprint {
        scan(root).unwrap().fingerprint
    }

    #[test]
    fn a_scan_hashes_every_enumerated_file_and_nothing_else() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());

        let scanned = scan(dir.path()).unwrap();

        let hashed: Vec<&str> = scanned.contents.keys().map(LogicalPath::as_str).collect();
        assert_eq!(
            hashed,
            vec![
                "common/technology/00_tech.txt",
                "common/technology/01_tech.txt",
                "descriptor.mod",
                "localisation/english/l_english.yml",
            ]
        );
        assert_eq!(
            scanned.contents[&LogicalPath::parse("common/technology/00_tech.txt").unwrap()],
            ContentHash::of(b"tech_a = {}\n")
        );
        assert_eq!(scanned.inventory.files.len(), scanned.contents.len());
    }

    #[test]
    fn rescanning_an_unchanged_tree_reproduces_the_fingerprint() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        assert_eq!(fingerprint_of(dir.path()), fingerprint_of(dir.path()));
    }

    #[test]
    fn the_same_content_under_a_different_root_has_the_same_fingerprint() {
        // Mod Installation identity comes from the location plus relative path; the
        // fingerprint describes content alone (docs/technical-design.md, "Installation
        // identity").
        let here = TempDir::new().unwrap();
        let there = TempDir::new().unwrap();
        staged_source(here.path());
        staged_source(there.path());
        assert_eq!(fingerprint_of(here.path()), fingerprint_of(there.path()));
    }

    #[test]
    fn an_edit_changes_the_fingerprint() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let before = fingerprint_of(dir.path());

        write(
            dir.path(),
            "common/technology/00_tech.txt",
            b"tech_a = { x }\n",
        );

        assert_ne!(fingerprint_of(dir.path()), before);
    }

    #[test]
    fn an_addition_changes_the_fingerprint() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let before = fingerprint_of(dir.path());

        write(
            dir.path(),
            "common/technology/02_tech.txt",
            b"tech_c = {}\n",
        );

        assert_ne!(fingerprint_of(dir.path()), before);
    }

    #[test]
    fn a_deletion_changes_the_fingerprint() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let before = fingerprint_of(dir.path());

        fs::remove_file(dir.path().join("common/technology/01_tech.txt")).unwrap();

        assert_ne!(fingerprint_of(dir.path()), before);
    }

    #[test]
    fn a_rename_changes_the_fingerprint_though_the_bytes_are_unchanged() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let before = fingerprint_of(dir.path());

        fs::rename(
            dir.path().join("common/technology/01_tech.txt"),
            dir.path().join("common/technology/09_tech.txt"),
        )
        .unwrap();

        assert_ne!(fingerprint_of(dir.path()), before);
    }

    #[test]
    fn a_case_only_rename_changes_the_fingerprint() {
        // The point of case-preserving logical identity: this must hold on macOS, whose
        // filesystem considers the two names the same file
        // (docs/technical-design.md, "Installation identity").
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let before = fingerprint_of(dir.path());

        fs::rename(
            dir.path().join("common/technology/01_tech.txt"),
            dir.path().join("common/technology/01_Tech.txt"),
        )
        .unwrap();

        let after = scan(dir.path()).unwrap();
        assert!(
            after
                .contents
                .keys()
                .any(|logical| logical.as_str() == "common/technology/01_Tech.txt"),
            "expected the renamed path, got {:?}",
            after.contents.keys().collect::<Vec<_>>()
        );
        assert_ne!(after.fingerprint, before);
    }

    #[test]
    fn editing_a_file_the_policy_excludes_does_not_change_the_fingerprint() {
        // Deliberate: referenced source assets invalidate a revision through the
        // manifest's referenced-source-asset set, not through this fingerprint, and the
        // build "does not hash unrelated large binary assets merely because they are
        // present" (docs/technical-design.md, "Source snapshot consistency").
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let before = fingerprint_of(dir.path());

        write(dir.path(), "gfx/models/ship.dds", b"different binary-ish");

        assert_eq!(fingerprint_of(dir.path()), before);
    }

    #[test]
    fn hashing_the_inventory_in_parallel_reproduces_the_fingerprint() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let scanned = scan(dir.path()).unwrap();

        let parallel: Vec<(LogicalPath, ContentHash)> = std::thread::scope(|scope| {
            let workers: Vec<_> = scanned
                .inventory
                .files
                .iter()
                .map(|file| {
                    let absolute = file.absolute_under(dir.path());
                    scope.spawn(move || {
                        (
                            file.logical.clone(),
                            ContentHash::of_file(&absolute).unwrap(),
                        )
                    })
                })
                .collect();
            // Joined in reverse, so completion order cannot be mistaken for input order.
            workers
                .into_iter()
                .rev()
                .map(|worker| worker.join().unwrap())
                .collect()
        });

        assert_eq!(
            SourceFingerprint::of(parallel).unwrap(),
            scanned.fingerprint
        );
    }

    #[test]
    fn an_unreadable_root_fails_the_scan() {
        let dir = TempDir::new().unwrap();
        assert!(matches!(
            scan(&dir.path().join("never-created")),
            Err(ScanError::Root(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_read_fails_the_scan_rather_than_shrinking_it() {
        // A fingerprint computed over a subset of the source is not a fingerprint of
        // that source. Failing loudly is what lets the caller retry the whole build.
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let secret = dir.path().join("common/technology/00_tech.txt");
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&secret).is_ok() {
            // Running with privileges that ignore the mode bits; nothing to observe.
            return;
        }

        // The kind distinguishes this from a file that vanished mid-scan: one is worth
        // retrying, the other is not.
        assert!(matches!(
            scan(dir.path()),
            Err(ScanError::Read {
                kind: std::io::ErrorKind::PermissionDenied,
                ..
            })
        ));
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[test]
    fn scan_errors_render_for_a_report() {
        let errors = [
            ScanError::Root(RootError::NotADirectory),
            ScanError::Read {
                logical: LogicalPath::parse("common/a.txt").unwrap(),
                kind: std::io::ErrorKind::PermissionDenied,
                detail: "permission denied".to_owned(),
            },
            ScanError::Duplicate(DuplicateLogicalPath {
                logical: LogicalPath::parse("common/a.txt").unwrap(),
            }),
        ];
        for error in errors {
            let rendered = error.to_string();
            assert!(!rendered.is_empty());
            assert!(!rendered.contains("ScanError"), "{rendered}");
        }
    }
}
