//! Build-lifetime Source Snapshots: the one observation every later phase consumes
//! (docs/technical-design.md, "Source module"; "Source snapshot consistency" steps 1-4).
//!
//! [`establish`] implements the optimistic protocol's first two steps: enumerate
//! deterministically, then read each file **once** into a bounded buffer and hash and
//! expose *those* bytes. Analysis parses exactly what was hashed, so a file that changes
//! mid-build cannot make the parse and the fingerprint describe different content — the
//! whole point of a snapshot over a sequence of live reads.
//!
//! Two decisions are worth stating here because the interface hides them:
//!
//! **Physical storage.** The MVP holds every enumerated script and localization file's
//! bytes in memory for the life of the build, and asset captures alongside them. The design
//! reserves the right to move that to private temporary storage after measurement, which is
//! why [`SourceBytes`] is an owned cheap-to-clone handle and not a borrow, and why nothing
//! outside this module may hold a `&[u8]` into a snapshot.
//!
//! Vanilla content is the load-bearing case, and it is not small. Measured on the local
//! Pegasus 4.4.6 install (STE-12, release build): 6,896 enumerated files, 217 MiB of
//! content, 259 MiB peak resident, 1.54 s to establish and 0.52 s to verify. A build holds
//! vanilla *and* the Target Mod at once, so ~320 MiB of resident source is the realistic
//! floor for a large mod. That is affordable for a desktop app today and is the number the
//! revision-bundle spike should weigh a temporary-file backing against; it is also why the
//! storage choice is hidden rather than published.
//!
//! **Liveness is a capability, not a flag.** Only a snapshot established from a live root
//! becomes a [`LiveSource`], and only a [`LiveSource`] can be asked to verify.

use crate::canonical::path::LogicalPath;
use crate::source::ScanError;
use crate::source::enumerate::{self, ObservationGaps, SourceFile};
use crate::source::fingerprint::{ContentHash, SourceFingerprint};
use crate::source::policy::FileFamily;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// Which contributor a snapshot observes. For the MVP's two-contributor scope this **is**
/// the snapshot's provenance.
///
/// Deliberately carries no `ModInstallationId`. That would create a `source -> discovery`
/// edge the design's permitted-edge list does not allow, and it would be a second
/// authority: the revision manifest already names the Mod Installation identifier at
/// revision scope. If a third contributor ever appears — a Playset member, a DLC layer —
/// a richer provenance type replaces this enum here, rather than a provenance field being
/// bolted onto every captured asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceKind {
    VanillaContent,
    TargetMod,
}

/// The exact bytes of one source file, as a cheaply cloneable owned handle.
///
/// Owned rather than `&[u8]` on purpose: the design reserves the right to move a snapshot's
/// physical storage to private temporary files after measurement, and a borrow would let
/// every caller depend on the storage choice this type exists to hide.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceBytes(Arc<[u8]>);

impl SourceBytes {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for SourceBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes.into())
    }
}

impl From<&[u8]> for SourceBytes {
    fn from(bytes: &[u8]) -> Self {
        Self(bytes.into())
    }
}

impl Deref for SourceBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SourceBytes {
    /// Length only. A derived `{:?}` of a multi-megabyte script file buries the assertion
    /// that failed under the content it was about.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SourceBytes({} bytes)", self.0.len())
    }
}

/// One enumerated file as the snapshot holds it: what the walk observed, plus the bytes and
/// the hash of those same bytes.
#[derive(Debug)]
struct CapturedFile {
    /// Kept whole rather than reduced to a family, because it is also the authority on how
    /// this logical path addresses the filesystem ([`SourceFile::absolute_under`]).
    file: SourceFile,
    hash: ContentHash,
    bytes: SourceBytes,
}

/// The live filesystem tree a snapshot was established from. Internal to `source`: outside
/// it, liveness is expressed by holding a [`LiveSource`], never by holding a path.
#[derive(Debug)]
pub(super) struct LiveRoot {
    /// The root as the caller gave it. Reads address the tree through it, so a directory
    /// link inside the tree keeps its lexical logical path.
    root: PathBuf,
    /// The resolved root: the containment boundary every asset read must stay inside.
    canonical: PathBuf,
}

impl LiveRoot {
    pub(super) fn path(&self) -> &Path {
        &self.root
    }
}

#[derive(Debug)]
enum Backing {
    Live(Arc<LiveRoot>),
}

/// A build-lifetime observation of one Mod Source: exact bytes for enumerated content,
/// frozen capture for referenced assets, the gaps the observation could not close, and the
/// one fingerprint that names all of it.
#[derive(Debug)]
pub struct SourceSnapshot {
    kind: SourceKind,
    backing: Backing,
    content: BTreeMap<LogicalPath, CapturedFile>,
    gaps: ObservationGaps,
    fingerprint: SourceFingerprint,
    /// One observation per logical path, successes and failures alike. See
    /// [`SourceSnapshot::read_asset`] for why failures are memoized too.
    assets: Mutex<BTreeMap<LogicalPath, AssetRead>>,
}

/// A frozen asset observation: the exact bytes this build will use, whatever the live tree
/// does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedAsset {
    pub kind: SourceKind,
    pub logical: LogicalPath,
    pub hash: ContentHash,
    pub bytes: SourceBytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetRead {
    Captured(CapturedAsset),
    Absent(AssetAbsence),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetAbsence {
    NotFound,
    /// The path resolved outside the canonical source root. A containment refusal, not an
    /// absence — kept distinct so it can never be read as "the mod didn't ship it".
    OutsideSourceRoot,
    /// `kind` and `detail` describe the host, so neither may reach a fingerprint; they are
    /// here because an Analysis Issue and a support log both want to say *why*.
    Unreadable {
        kind: io::ErrorKind,
        detail: String,
    },
}

impl SourceSnapshot {
    pub fn kind(&self) -> SourceKind {
        self.kind
    }

    /// The `/v3` fingerprint over exactly the captured logical content and the gaps.
    pub fn fingerprint(&self) -> SourceFingerprint {
        self.fingerprint
    }

    /// What this observation could not see. The single authority on completeness:
    /// `gaps().is_empty()` is what [`Established::Complete`] means.
    pub fn gaps(&self) -> &ObservationGaps {
        &self.gaps
    }

    /// Enumerated logical paths in canonical byte order.
    pub fn paths(&self) -> impl ExactSizeIterator<Item = &LogicalPath> {
        self.content.keys()
    }

    /// The family enumeration decided for an enumerated path, or `None` when the path is
    /// not enumerated content. Nothing downstream re-derives a family from a path.
    pub fn family(&self, logical: &LogicalPath) -> Option<FileFamily> {
        self.content
            .get(logical)
            .map(|captured| captured.file.family)
    }

    /// The exact bytes that were hashed, or `None` when the path is not part of this
    /// snapshot's content.
    ///
    /// `None` is "not in this snapshot", never "empty": an enumerated empty file reads back
    /// as zero bytes.
    pub fn read(&self, logical: &LogicalPath) -> Option<SourceBytes> {
        self.content
            .get(logical)
            .map(|captured| captured.bytes.clone())
    }

    /// Resolves a referenced source asset, freezing the result for the rest of the build.
    ///
    /// **One observation per logical path, successes and failures alike.** Two reads in one
    /// build must not disagree because the tree moved between them. The design freezes on
    /// first success; memoizing the failure is the same rule applied to the same question,
    /// and it is what keeps "the mod did not ship this" a stable fact that an Analysis Issue
    /// can be attached to.
    ///
    /// Concurrency: analysis parallelises, so the memo is behind a `Mutex`. The first read
    /// for a key holds the lock across its I/O, which serialises unrelated first reads.
    /// Per-key entries (a map of `OnceLock`s, or a two-phase insert) are the fix if that
    /// ever measures; one lock keeps the "exactly one observation" invariant obvious until
    /// then.
    pub fn read_asset(&self, logical: &LogicalPath) -> AssetRead {
        let mut assets = self.assets.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(observed) = assets.get(logical) {
            return observed.clone();
        }
        let observed = self.observe_asset(logical);
        assets.insert(logical.clone(), observed.clone());
        observed
    }

    /// Every asset this build froze, in canonical logical-path order — the revision
    /// manifest's referenced-source-asset input set, and what final verification re-reads.
    ///
    /// Absences are deliberately not here. A file the mod never shipped is an Analysis
    /// Issue, not an input whose bytes a later build could compare against.
    pub fn captured_assets(&self) -> Vec<CapturedAsset> {
        let assets = self.assets.lock().unwrap_or_else(PoisonError::into_inner);
        // A `BTreeMap` iterates in key order, so the canonical order is the collection's.
        assets
            .values()
            .filter_map(|read| match read {
                AssetRead::Captured(asset) => Some(asset.clone()),
                AssetRead::Absent(_) => None,
            })
            .collect()
    }

    fn observe_asset(&self, logical: &LogicalPath) -> AssetRead {
        // Enumerated content is never re-read. The snapshot already holds the exact bytes
        // it hashed; going back to disk would give one logical path two authorities for its
        // bytes inside one build. The capture still joins `captured_assets`, because a
        // script file consumed as an asset is a referenced source-asset input like any
        // other.
        if let Some(captured) = self.content.get(logical) {
            return AssetRead::Captured(CapturedAsset {
                kind: self.kind,
                logical: logical.clone(),
                hash: captured.hash,
                bytes: captured.bytes.clone(),
            });
        }
        let observed = match &self.backing {
            Backing::Live(root) => read_live_asset(root, &self.live_path(root, logical)),
        };
        match observed {
            Ok(bytes) => AssetRead::Captured(CapturedAsset {
                kind: self.kind,
                logical: logical.clone(),
                hash: ContentHash::of(&bytes),
                bytes,
            }),
            Err(absence) => AssetRead::Absent(absence),
        }
    }

    /// Where a logical path lives under a live root.
    ///
    /// Enumerated content is addressed through the raw names the walk observed, because
    /// identity is NFC and a filesystem need not agree. An asset reference has no other
    /// spelling — it arrives from script text, not from a directory listing — so it is
    /// joined as written.
    pub(super) fn live_path(&self, root: &LiveRoot, logical: &LogicalPath) -> PathBuf {
        match self.content.get(logical) {
            Some(captured) => captured.file.absolute_under(&root.root),
            None => {
                let mut path = root.root.clone();
                for component in logical.components() {
                    path.push(component);
                }
                path
            }
        }
    }
}

/// A snapshot established from a live filesystem root — the only thing that can be verified
/// against one.
#[derive(Debug)]
pub struct LiveSource {
    snapshot: SourceSnapshot,
    /// The same `LiveRoot` the snapshot's backing holds, shared rather than copied. Sharing
    /// it is what lets `verify` take a root without an impossible "memory-backed live
    /// source" arm to discharge, and keeps one authority for where the tree is.
    root: Arc<LiveRoot>,
}

impl LiveSource {
    pub fn snapshot(&self) -> &SourceSnapshot {
        &self.snapshot
    }

    pub(super) fn live_root(&self) -> &LiveRoot {
        &self.root
    }
}

/// The outcome of establishing a snapshot from a live root.
///
/// Both arms carry a usable observation: an incomplete one is publishable, and its gaps
/// become Analysis Issues downstream — "evidence absent" is a documented Incomplete
/// Documentation condition, not a fatal one. The enum's only job is that a consumer cannot
/// reach a snapshot without deciding what incompleteness means for the product.
/// [`SourceSnapshot::gaps`] stays the single authority on *what* was missed; this is only
/// the decision point.
#[derive(Debug)]
pub enum Established {
    Complete(LiveSource),
    Incomplete(LiveSource),
}

/// Establishment fails for exactly the reasons a hash-only [`scan`](crate::source::scan)
/// does — the root, an unreadable enumerated file, or the documented-unreachable duplicate —
/// so it is that union under the name establishment's callers use, rather than a second
/// copy to keep in step with it.
pub type EstablishError = ScanError;

/// Establishes a build-lifetime snapshot of `root`.
///
/// Reads every enumerated file once and keeps those exact bytes; a file that cannot be read
/// fails the whole establishment, because a snapshot over a subset of the source would
/// claim to describe the source (the same rule `scan` applies).
pub fn establish(kind: SourceKind, root: &Path) -> Result<Established, EstablishError> {
    let inventory = enumerate::enumerate(root)?;
    // Resolved once here and kept: it is the containment boundary for every later asset
    // read, and re-resolving per read would let a boundary move mid-build.
    let canonical = fs::canonicalize(root).map_err(|error| {
        EstablishError::Root(enumerate::RootError::Unreadable {
            kind: error.kind(),
            detail: error.to_string(),
        })
    })?;
    let gaps = inventory.gaps();

    let mut content = BTreeMap::new();
    for file in inventory.files {
        // Snapshot protocol step 2: one read into a bounded buffer, then hash and expose
        // *those* bytes. Hashing from a second read would be a second observation, which is
        // the inconsistency the snapshot exists to remove.
        let bytes = fs::read(file.absolute_under(root)).map_err(|error| EstablishError::Read {
            logical: file.logical.clone(),
            kind: error.kind(),
            detail: error.to_string(),
        })?;
        let bytes = SourceBytes::from(bytes);
        let hash = ContentHash::of(&bytes);
        content.insert(file.logical.clone(), CapturedFile { file, hash, bytes });
    }

    let fingerprint = SourceFingerprint::of(
        content
            .iter()
            .map(|(logical, captured)| (logical.clone(), captured.hash)),
        &gaps,
    )
    .map_err(EstablishError::Duplicate)?;

    let complete = gaps.is_empty();
    let root = Arc::new(LiveRoot {
        root: root.to_path_buf(),
        canonical,
    });
    let source = LiveSource {
        snapshot: SourceSnapshot {
            kind,
            backing: Backing::Live(Arc::clone(&root)),
            content,
            gaps,
            fingerprint,
            assets: Mutex::new(BTreeMap::new()),
        },
        root,
    };
    Ok(match complete {
        true => Established::Complete(source),
        false => Established::Incomplete(source),
    })
}

/// Reads one asset from a live root, refusing anything that resolves outside it.
///
/// Containment is the rule `enumerate` applies to links, applied to a path the caller
/// supplied instead of one a walk found. [`LogicalPath`] already forbids `..`, an absolute
/// prefix, and backslashes lexically; resolving catches what lexical rules cannot — a link
/// inside the tree pointing out of it. The read then addresses the resolved path, so the
/// bytes come from the location containment was decided about.
pub(super) fn read_live_asset(root: &LiveRoot, path: &Path) -> Result<SourceBytes, AssetAbsence> {
    let resolved = match fs::canonicalize(path) {
        Ok(resolved) => resolved,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(AssetAbsence::NotFound);
        }
        Err(error) => {
            return Err(AssetAbsence::Unreadable {
                kind: error.kind(),
                detail: error.to_string(),
            });
        }
    };
    if !resolved.starts_with(&root.canonical) {
        return Err(AssetAbsence::OutsideSourceRoot);
    }
    fs::read(&resolved)
        .map(SourceBytes::from)
        .map_err(|error| AssetAbsence::Unreadable {
            kind: error.kind(),
            detail: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::scan;
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

    fn path(raw: &str) -> LogicalPath {
        LogicalPath::parse(raw).unwrap()
    }

    /// Both arms carry a live source; which arm it was is asserted where that is the point.
    fn source_of(established: Established) -> LiveSource {
        match established {
            Established::Complete(source) | Established::Incomplete(source) => source,
        }
    }

    fn established(root: &Path) -> LiveSource {
        source_of(establish(SourceKind::TargetMod, root).unwrap())
    }

    #[test]
    fn a_snapshot_holds_the_exact_bytes_it_hashed() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());

        let snapshot = established(dir.path());
        let snapshot = snapshot.snapshot();

        let paths: Vec<&str> = snapshot.paths().map(LogicalPath::as_str).collect();
        assert_eq!(
            paths,
            vec![
                "common/technology/00_tech.txt",
                "common/technology/01_tech.txt",
                "descriptor.mod",
                "localisation/english/l_english.yml",
            ]
        );
        assert_eq!(
            snapshot
                .read(&path("common/technology/00_tech.txt"))
                .unwrap()
                .as_slice(),
            b"tech_a = {}\n"
        );
        assert_eq!(
            snapshot.family(&path("localisation/english/l_english.yml")),
            Some(FileFamily::Localization)
        );
        assert_eq!(
            snapshot.family(&path("common/technology/00_tech.txt")),
            Some(FileFamily::Script)
        );
        // Not in the snapshot is not the same as empty.
        assert_eq!(snapshot.read(&path("gfx/models/ship.dds")), None);
        assert_eq!(snapshot.family(&path("gfx/models/ship.dds")), None);
        assert_eq!(snapshot.kind(), SourceKind::TargetMod);
    }

    #[test]
    fn a_snapshot_and_a_hash_only_scan_agree_on_the_fingerprint() {
        // Establishment reads bytes and `scan` streams them; if the two disagreed, the
        // pre-publication verification would report a change on every build.
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        assert_eq!(
            established(dir.path()).snapshot().fingerprint(),
            scan(dir.path()).unwrap().fingerprint
        );
    }

    #[test]
    fn establishment_reports_complete_exactly_when_there_are_no_gaps() {
        // The enum's contract: it is a decision point, not a second authority. The
        // collision arm is exercised at `classify_entries`' pure seam instead of here,
        // because APFS is normalization-insensitive and cannot stage two names that
        // normalize alike (D-111).
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let clean = establish(SourceKind::TargetMod, dir.path()).unwrap();
        assert!(matches!(clean, Established::Complete(_)));
        assert!(source_of(clean).snapshot().gaps().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_rejection_makes_the_observation_incomplete_and_is_exposed() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        std::os::unix::fs::symlink("nowhere.txt", dir.path().join("common/dangling.txt")).unwrap();

        let established = establish(SourceKind::TargetMod, dir.path()).unwrap();

        assert!(matches!(established, Established::Incomplete(_)));
        let source = source_of(established);
        let gaps = source.snapshot().gaps();
        assert!(!gaps.is_empty());
        assert_eq!(gaps.rejected.len(), 1);
        assert_eq!(gaps.rejected[0].raw_label, "common/dangling.txt");
        assert!(gaps.collisions.is_empty());
        // The gaps are a report *and* identity: the snapshot still holds every file that
        // survived, so it is publishable.
        assert_eq!(source.snapshot().paths().len(), 4);
    }

    #[cfg(unix)]
    #[test]
    fn removing_a_dangling_symlink_changes_the_snapshot_fingerprint() {
        // THE REGRESSION THIS TICKET EXISTS FOR. Deleting a dangling link removes a
        // rejection and changes no enumerated file. Under a content-only fingerprint the
        // repaired tree is byte-identical to the broken one, so a revision built while the
        // link dangled verifies as unchanged and keeps its stale "evidence absent" issue
        // for good.
        //
        // Negative control (run during STE-12, per AGENTS.md's rule that a gate must be
        // shown to detect its failure): dropping the gap projection from
        // `SourceFingerprint::of` turns this assertion red with two equal digests.
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let link = dir.path().join("common/dangling.txt");
        std::os::unix::fs::symlink("nowhere.txt", &link).unwrap();

        let broken = established(dir.path());
        let broken_paths: Vec<LogicalPath> = broken.snapshot().paths().cloned().collect();
        let broken_fingerprint = broken.snapshot().fingerprint();

        fs::remove_file(&link).unwrap();
        let repaired = established(dir.path());

        assert_eq!(
            repaired.snapshot().paths().cloned().collect::<Vec<_>>(),
            broken_paths,
            "no enumerated file may have changed, or the test proves nothing"
        );
        for logical in &broken_paths {
            assert_eq!(
                repaired.snapshot().read(logical),
                broken.snapshot().read(logical)
            );
        }
        assert_ne!(repaired.snapshot().fingerprint(), broken_fingerprint);
    }

    #[cfg(unix)]
    #[test]
    fn the_same_defect_under_two_roots_has_the_same_fingerprint() {
        // Host independence. The escape targets are distinct absolute paths under distinct
        // outside directories, so a fingerprint that quoted the resolved target — or the
        // root — would differ here even though the two mods are the same mod.
        fn stage(root: &Path, outside: &Path) {
            staged_source(root);
            fs::write(outside.join("foreign.txt"), "foreign\n").unwrap();
            std::os::unix::fs::symlink(
                outside.join("foreign.txt"),
                root.join("common/escaped.txt"),
            )
            .unwrap();
        }
        let here = TempDir::new().unwrap();
        let here_outside = TempDir::new().unwrap();
        let there = TempDir::new().unwrap();
        let there_outside = TempDir::new().unwrap();
        stage(here.path(), here_outside.path());
        stage(there.path(), there_outside.path());
        assert_ne!(here_outside.path(), there_outside.path());

        let first = established(here.path());
        let second = established(there.path());

        // The gap reports differ, because the report names where the link went...
        assert_ne!(first.snapshot().gaps(), second.snapshot().gaps());
        // ...and the identity does not.
        assert_eq!(
            first.snapshot().fingerprint(),
            second.snapshot().fingerprint()
        );
    }

    #[test]
    fn a_first_asset_read_freezes_bytes_against_a_later_edit() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established(dir.path());
        let asset = path("gfx/models/ship.dds");

        let first = source.snapshot().read_asset(&asset);
        let AssetRead::Captured(captured) = &first else {
            panic!("expected a capture, got {first:?}");
        };
        assert_eq!(captured.bytes.as_slice(), b"binary-ish");
        assert_eq!(captured.hash, ContentHash::of(b"binary-ish"));
        assert_eq!(captured.kind, SourceKind::TargetMod);

        write(dir.path(), "gfx/models/ship.dds", b"edited under the build");
        assert_eq!(source.snapshot().read_asset(&asset), first);

        fs::remove_file(dir.path().join("gfx/models/ship.dds")).unwrap();
        assert_eq!(source.snapshot().read_asset(&asset), first);
    }

    #[test]
    fn a_memoized_absence_stays_absent_after_the_file_appears() {
        // The same rule as freezing a success. An Analysis Issue was already attached to
        // "the mod did not ship this"; a later read that found the file would make two
        // stages of one build disagree about the same question.
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established(dir.path());
        let asset = path("gfx/models/late.dds");

        assert_eq!(
            source.snapshot().read_asset(&asset),
            AssetRead::Absent(AssetAbsence::NotFound)
        );
        write(dir.path(), "gfx/models/late.dds", b"arrived late");
        assert_eq!(
            source.snapshot().read_asset(&asset),
            AssetRead::Absent(AssetAbsence::NotFound)
        );
        assert!(source.snapshot().captured_assets().is_empty());
    }

    #[test]
    fn an_asset_read_of_enumerated_content_is_served_from_the_snapshot() {
        // One authority for a file's bytes: the file is deleted before the read, and the
        // read still succeeds with the bytes the snapshot hashed.
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established(dir.path());
        let logical = path("common/technology/00_tech.txt");
        fs::remove_file(dir.path().join("common/technology/00_tech.txt")).unwrap();

        let read = source.snapshot().read_asset(&logical);

        let AssetRead::Captured(captured) = read else {
            panic!("enumerated content must not be re-read from disk");
        };
        assert_eq!(captured.bytes.as_slice(), b"tech_a = {}\n");
        assert_eq!(
            source
                .snapshot()
                .captured_assets()
                .iter()
                .map(|asset| asset.logical.as_str())
                .collect::<Vec<_>>(),
            vec!["common/technology/00_tech.txt"]
        );
    }

    #[test]
    fn captured_assets_are_in_canonical_logical_path_order() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        write(dir.path(), "gfx/models/zeta.dds", b"z");
        write(dir.path(), "gfx/models/alpha.dds", b"a");
        write(dir.path(), "gfx/models/Zeta.dds", b"Z");
        let source = established(dir.path());

        for name in ["zeta", "alpha", "Zeta"] {
            source
                .snapshot()
                .read_asset(&path(&format!("gfx/models/{name}.dds")));
        }

        assert_eq!(
            source
                .snapshot()
                .captured_assets()
                .iter()
                .map(|asset| asset.logical.as_str())
                .collect::<Vec<_>>(),
            // Byte order, so uppercase `Z` (0x5a) precedes lowercase `a` (0x61).
            vec![
                "gfx/models/Zeta.dds",
                "gfx/models/alpha.dds",
                "gfx/models/zeta.dds",
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_asset_resolving_outside_the_root_is_a_containment_refusal() {
        // Not `NotFound`: the file exists and is readable, and the build refused it. The
        // two must stay distinguishable, or a refusal reads as "the mod didn't ship it".
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("foreign.dds"), b"foreign").unwrap();
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        std::os::unix::fs::symlink(
            outside.path().join("foreign.dds"),
            dir.path().join("gfx/models/escaped.dds"),
        )
        .unwrap();
        let source = established(dir.path());

        assert_eq!(
            source
                .snapshot()
                .read_asset(&path("gfx/models/escaped.dds")),
            AssetRead::Absent(AssetAbsence::OutsideSourceRoot)
        );
        assert!(source.snapshot().captured_assets().is_empty());
    }

    #[test]
    fn an_asset_that_is_a_directory_is_unreadable_rather_than_absent() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established(dir.path());

        assert!(matches!(
            source.snapshot().read_asset(&path("gfx/models")),
            AssetRead::Absent(AssetAbsence::Unreadable { .. })
        ));
    }

    #[test]
    fn establishment_fails_rather_than_shrinking_the_source() {
        let dir = TempDir::new().unwrap();
        assert!(matches!(
            establish(
                SourceKind::VanillaContent,
                &dir.path().join("never-created")
            ),
            Err(EstablishError::Root(_))
        ));
    }

    #[test]
    fn source_bytes_debug_reports_a_length_not_a_payload() {
        let bytes = SourceBytes::from(vec![0u8; 4096]);
        let rendered = format!("{bytes:?}");
        assert_eq!(rendered, "SourceBytes(4096 bytes)");
    }
}
