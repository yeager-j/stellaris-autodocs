//! Sole owner of complete Mod Source traversal and content identity: deterministic
//! enumeration, logical-path normalization and escape rejection, hashing, fingerprints,
//! build-lifetime Source Snapshots, and final live-source verification
//! (docs/technical-design.md, "Source module"). Populated in Phase 2.
//!
//! [`snapshot::establish`] is the module's product surface: it turns a live root into a
//! build-lifetime [`SourceSnapshot`] that holds the exact bytes analysis parses, answers
//! asset requests with frozen capture, and — through [`LiveSource::verify`] — can be
//! checked against the live tree immediately before publication. Every later phase
//! consumes `&SourceSnapshot` and nothing else.
//!
//! [`scan`] is the hash-only half of that protocol: enumerate, hash, fingerprint, keeping
//! no bytes. Establishment does not use it (it must keep what it read), but verification
//! does, which is the design's deliberate second read of the live filesystem
//! (docs/technical-design.md, "Source snapshot consistency").

pub mod enumerate;
pub mod fingerprint;
pub mod policy;
pub mod snapshot;
pub mod verify;

#[cfg(feature = "test-support")]
pub mod fixture;

pub use enumerate::{
    FileCollision, ObservationGaps, RejectedFile, RejectionReason, RootError, SourceFile,
    SourceInventory,
};
pub use fingerprint::{ContentHash, DuplicateLogicalPath, HashParseError, SourceFingerprint};
pub use policy::{ENUMERATION_POLICY_VERSION, FileFamily};
pub use snapshot::{
    AssetAbsence, AssetRead, CapturedAsset, EstablishError, Established, LiveSource, SourceBytes,
    SourceKind, SourceSnapshot, establish,
};
pub use verify::{
    AppearedAsset, AssetChange, FingerprintMismatch, LiveVerification, ObservedAsset,
    ObservedFingerprint, SourceChange, UnscannableSource,
};

use crate::canonical::path::LogicalPath;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

/// One hash-only observation of a Mod Source: what was enumerated, what it contained,
/// what it could not see, and the one value that names all of it.
///
/// The fingerprint covers the enumerated content **and** `inventory.gaps()`, so an
/// observation that hit a collision or a rejection has a different identity from the same
/// content observed cleanly. A caller therefore does not have to remember to ask whether a
/// fingerprint is trustworthy; it has to decide what an incomplete observation means for
/// the product, which [`Established`] forces at the point a snapshot is built.
#[derive(Debug)]
pub struct SourceScan {
    pub inventory: SourceInventory,
    /// Keyed by the logical paths of `inventory.files`, one hash per enumerated file.
    pub contents: BTreeMap<LogicalPath, ContentHash>,
    /// Derived from `contents` together with `inventory.gaps()`.
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
/// source fingerprint from that content and the walk's gaps.
///
/// Keeps no bytes: this is the identity half of the snapshot protocol, used by
/// [`LiveSource::verify`] for the pre-publication re-read. Use [`establish`] when the
/// bytes themselves are wanted.
///
/// Files are read through the raw names enumeration observed, never through the
/// NFC-normalized identity: on a normalization-sensitive filesystem the identity does not
/// address the file.
pub fn scan(root: &Path) -> Result<SourceScan, ScanError> {
    let inventory = enumerate::enumerate(root)?;
    // One entry per enumerated file, deliberately *not* deduplicated. `contents` is keyed by
    // logical path and would collapse a repeat into silent last-writer-wins before
    // `SourceFingerprint::of` could see it — which is what made the duplicate refusal, and
    // `ScanError::Duplicate` with it, structurally dead. Feeding `of` the un-collapsed list
    // is what checks `classify_entries`' one-file-per-logical-path invariant instead of
    // assuming it.
    let mut hashed = Vec::with_capacity(inventory.files.len());
    let mut contents = BTreeMap::new();
    for file in &inventory.files {
        let content =
            ContentHash::of_file(&file.absolute_under(root)).map_err(|error| ScanError::Read {
                logical: file.logical.clone(),
                kind: error.kind(),
                detail: error.to_string(),
            })?;
        hashed.push((file.logical.clone(), content));
        contents.insert(file.logical.clone(), content);
    }
    let fingerprint =
        SourceFingerprint::of(hashed, &inventory.gaps()).map_err(ScanError::Duplicate)?;
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
            SourceFingerprint::of(parallel, &scanned.inventory.gaps()).unwrap(),
            scanned.fingerprint
        );
    }

    #[cfg(unix)]
    #[test]
    fn removing_a_dangling_symlink_changes_the_fingerprint() {
        // The regression /v3 exists for, at the `scan` level. No enumerated file changes:
        // the link was never a file. Under a content-only fingerprint the two observations
        // are identical, so a revision built while the link dangled would verify as
        // unchanged and keep its stale "evidence absent" Analysis Issue forever.
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        std::os::unix::fs::symlink("nowhere.txt", dir.path().join("common/dangling.txt")).unwrap();

        let broken = scan(dir.path()).unwrap();
        assert_eq!(broken.inventory.rejected.len(), 1);

        fs::remove_file(dir.path().join("common/dangling.txt")).unwrap();
        let repaired = scan(dir.path()).unwrap();

        assert_eq!(repaired.contents, broken.contents);
        assert!(repaired.inventory.gaps().is_empty());
        assert_ne!(repaired.fingerprint, broken.fingerprint);
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
