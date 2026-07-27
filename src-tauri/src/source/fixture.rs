//! Memory-backed fixture corpora: realistic Source Snapshots with no Steam installation,
//! no host paths, and no filesystem traversal (docs/technical-design.md, "Source module";
//! "Verification architecture").
//!
//! Source-owned rather than a member of `testsupport`, because a fixture snapshot must be
//! built by the same construction and fingerprint path a live one is; a second builder
//! elsewhere would be a second authority on what a snapshot can contain. `testsupport` keeps
//! the process-scoped isolation helpers and points here.
//!
//! Gated behind the `test-support` feature (D-107), so a production build cannot construct
//! one — the same gate that keeps [`SourceSnapshot`]'s memory backing out of a shipped
//! binary.
//!
//! There is deliberately **no `from_directory` loader**. That would reintroduce exactly the
//! live traversal these fixtures exist to avoid. Realistic corpora come from `fixtures/`
//! through `include_bytes!`, which is a compile-time read, or from inline literals.

use crate::canonical::path::{LogicalPath, PathError};
use crate::source::enumerate::{FileCollision, ObservationGaps};
use crate::source::policy::{self, FileFamily};
use crate::source::snapshot::{SourceBytes, SourceKind, SourceSnapshot};
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A fixture Source Snapshot under construction.
///
/// The builder is fallible but its steps are not: a bad entry is remembered and surfaces
/// from [`FixtureCorpus::build`], so a corpus still reads as one expression.
pub struct FixtureCorpus {
    kind: SourceKind,
    files: BTreeMap<LogicalPath, (FileFamily, SourceBytes)>,
    assets: BTreeMap<LogicalPath, SourceBytes>,
    gaps: ObservationGaps,
    /// The first refusal a single step could see. Later ones are dropped: the first is the
    /// one a test author has to fix, and a list of consequences would bury it. Refusals that
    /// need the whole corpus are decided in [`FixtureCorpus::build`] and reported after this
    /// one, because a step that could not see the problem cannot be blamed for it.
    refused: Option<FixtureError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixtureError {
    /// Not a logical path at all.
    InvalidPath { entry: String, error: PathError },
    /// The enumeration policy excludes this path, so no real snapshot could ever hold it as
    /// content. Refused rather than silently ignored: a fixture that lies about what a
    /// snapshot can contain is worse than no fixture.
    FileExcludedByPolicy { logical: LogicalPath },
    /// The policy *selects* this path, so a real snapshot holds it as enumerated content and
    /// serves asset reads from those bytes. Seeding it as an asset would be a second
    /// authority for one file's bytes.
    AssetSelectedByPolicy { logical: LogicalPath },
    /// One logical path offered twice. Same rule as [`SourceFingerprint::of`]'s duplicate
    /// refusal: a corpus that names a path twice has lost track of itself.
    ///
    /// [`SourceFingerprint::of`]: crate::source::SourceFingerprint::of
    Duplicate { logical: LogicalPath },
    /// A declared collision named fewer than two raw spellings. One raw entry that
    /// normalizes to a logical path is a file, not a collision, and `classify_entries`
    /// cannot produce this shape.
    NotACollision { logical: LogicalPath },
}

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath { entry, error } => {
                write!(f, "fixture path {entry:?} is not a logical path: {error}")
            }
            Self::FileExcludedByPolicy { logical } => write!(
                f,
                "the enumeration policy excludes {logical}, so a Source Snapshot cannot \
                 contain it as content"
            ),
            Self::AssetSelectedByPolicy { logical } => write!(
                f,
                "the enumeration policy selects {logical}, so it is enumerated content \
                 rather than an asset; add it with `with_file`"
            ),
            Self::Duplicate { logical } => {
                write!(f, "the fixture corpus already contains {logical}")
            }
            Self::NotACollision { logical } => write!(
                f,
                "a collision at {logical} needs at least two raw spellings; one raw entry \
                 normalizing to a logical path is a file"
            ),
        }
    }
}

impl std::error::Error for FixtureError {}

impl FixtureCorpus {
    pub fn new(kind: SourceKind) -> Self {
        Self {
            kind,
            files: BTreeMap::new(),
            assets: BTreeMap::new(),
            gaps: ObservationGaps::default(),
            refused: None,
        }
    }

    /// Adds one enumerated file, applying the real enumeration policy.
    pub fn with_file(mut self, logical: &str, bytes: &[u8]) -> Self {
        match self.parse(logical) {
            Ok(logical) => match policy::family_for(&logical) {
                Some(family) => self.insert_file(logical, family, bytes),
                None => self.refuse(FixtureError::FileExcludedByPolicy { logical }),
            },
            Err(error) => self.refuse(error),
        }
        self
    }

    /// Seeds the bytes a lazy asset read will find. The fixture equivalent of the file being
    /// present on disk: an unseeded path reads back as [`AssetAbsence::NotFound`].
    ///
    /// [`AssetAbsence::NotFound`]: crate::source::AssetAbsence::NotFound
    pub fn with_asset(mut self, logical: &str, bytes: &[u8]) -> Self {
        match self.parse(logical) {
            Ok(logical) if policy::family_for(&logical).is_some() => {
                self.refuse(FixtureError::AssetSelectedByPolicy { logical });
            }
            Ok(logical) => self.insert_asset(logical, bytes),
            Err(error) => self.refuse(error),
        }
        self
    }

    /// Declares that two or more raw entries normalized to one logical path, making the
    /// fixture an **incomplete** observation.
    ///
    /// The one gap shape a filesystem here cannot stage: APFS is normalization- and
    /// case-insensitive, so colliding names cannot be created on the development machine
    /// (D-111). Declaring it is what lets the collision rules — the gap's effect on the
    /// fingerprint, and `read_asset`'s refusal to pick a winner — be exercised at all.
    /// Every check a collision needs is a property of the whole corpus rather than of this
    /// call, so this step only records the declaration; [`FixtureCorpus::build`] validates
    /// it. See `validate_collisions` for why that is not a stylistic choice.
    pub fn with_collision(mut self, logical: &str, raw_labels: &[&str]) -> Self {
        match self.parse(logical) {
            Ok(logical) => {
                let mut raw_labels: Vec<String> =
                    raw_labels.iter().map(|label| (*label).to_owned()).collect();
                raw_labels.sort();
                self.gaps.collisions.push(FileCollision {
                    logical,
                    raw_labels,
                });
            }
            Err(error) => self.refuse(error),
        }
        self
    }

    /// Builds the snapshot, or reports the first entry the corpus refused.
    pub fn build(mut self) -> Result<SourceSnapshot, FixtureError> {
        if let Some(error) = self.refused {
            return Err(error);
        }
        validate_collisions(&self.gaps.collisions, &self.files)?;
        // `classify_entries` emits collisions in logical-path order because it drains a
        // `BTreeMap`. The digest re-sorts, but `gaps()` is exposed, and a fixture whose
        // report order differed from a live one's would be a difference a test could see.
        self.gaps
            .collisions
            .sort_by(|left, right| left.logical.cmp(&right.logical));
        Ok(SourceSnapshot::in_memory(
            self.kind,
            self.files,
            self.assets,
            self.gaps,
        ))
    }

    fn parse(&self, logical: &str) -> Result<LogicalPath, FixtureError> {
        LogicalPath::parse(logical).map_err(|error| FixtureError::InvalidPath {
            entry: logical.to_owned(),
            error,
        })
    }

    fn insert_file(&mut self, logical: LogicalPath, family: FileFamily, bytes: &[u8]) {
        match self.files.entry(logical.clone()) {
            Entry::Vacant(slot) => {
                slot.insert((family, SourceBytes::from(bytes)));
            }
            Entry::Occupied(_) => self.refuse(FixtureError::Duplicate { logical }),
        }
    }

    fn insert_asset(&mut self, logical: LogicalPath, bytes: &[u8]) {
        match self.assets.entry(logical.clone()) {
            Entry::Vacant(slot) => {
                slot.insert(SourceBytes::from(bytes));
            }
            Entry::Occupied(_) => self.refuse(FixtureError::Duplicate { logical }),
        }
    }

    fn refuse(&mut self, error: FixtureError) {
        self.refused.get_or_insert(error);
    }
}

/// Checks declared collisions against the corpus they belong to.
///
/// Every one of these is a property of the whole corpus, not of the `with_collision` call,
/// which is why guarding in place was wrong rather than merely untidy: `with_collision`
/// could consult `self.files`, but `with_file` never consults `self.gaps`, so
/// `with_file(p).with_collision(p)` was refused while `with_collision(p).with_file(p)`
/// built — and that second corpus answers `read_asset(p)` with `Captured`, where a live
/// snapshot answers `Absent(Collision)`. A fixture that disagrees with a live tree about the
/// collision rule is exactly what `FixtureError`'s own contract forbids. Deciding here makes
/// all three checks order-independent by construction rather than by guard.
fn validate_collisions(
    collisions: &[FileCollision],
    files: &BTreeMap<LogicalPath, (FileFamily, SourceBytes)>,
) -> Result<(), FixtureError> {
    let mut declared = BTreeSet::new();
    for collision in collisions {
        let logical = collision.logical.clone();
        if collision.raw_labels.len() < 2 {
            return Err(FixtureError::NotACollision { logical });
        }
        // `classify_entries` only ever considers paths the policy selects, so a collision at
        // an excluded path is not an observation any walk could make — the same rule, and
        // the same refusal, `with_file` applies.
        if policy::family_for(&logical).is_none() {
            return Err(FixtureError::FileExcludedByPolicy { logical });
        }
        // Enumeration emits a surviving file or a collision for a path, never both: a
        // collided path has no winner, which is the whole point of the collision.
        if files.contains_key(&logical) {
            return Err(FixtureError::Duplicate { logical });
        }
        // Collisions are keyed by logical path in `classify_entries`, so one path collides
        // at most once. Two declarations would also encode as two items in the digest.
        if !declared.insert(logical.clone()) {
            return Err(FixtureError::Duplicate { logical });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::snapshot::{AssetAbsence, AssetRead};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    /// Real corpus content, read at compile time. `include_bytes!` is what keeps "no runtime
    /// traversal" true while the bytes stay the ones a Stellaris mod actually ships.
    const DESCRIPTOR: &[u8] = include_bytes!("../../../fixtures/oracle/target/descriptor.mod");
    const TECHNOLOGY: &[u8] =
        include_bytes!("../../../fixtures/oracle/target/common/technology/zz_oracle_tech.txt");

    fn path(raw: &str) -> LogicalPath {
        LogicalPath::parse(raw).unwrap()
    }

    fn oracle_target() -> FixtureCorpus {
        FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", DESCRIPTOR)
            .with_file("common/technology/zz_oracle_tech.txt", TECHNOLOGY)
    }

    #[test]
    fn a_corpus_of_real_fixture_content_keeps_its_paths_and_exact_bytes() {
        let snapshot = oracle_target().build().unwrap();

        assert_eq!(
            snapshot
                .paths()
                .map(LogicalPath::as_str)
                .collect::<Vec<_>>(),
            vec!["common/technology/zz_oracle_tech.txt", "descriptor.mod"]
        );
        assert_eq!(
            snapshot
                .read(&path("common/technology/zz_oracle_tech.txt"))
                .unwrap()
                .as_slice(),
            TECHNOLOGY
        );
        assert!(TECHNOLOGY.len() > 100, "the fixture must be real content");
        assert_eq!(
            snapshot.family(&path("descriptor.mod")),
            Some(FileFamily::Script)
        );
        assert_eq!(snapshot.kind(), SourceKind::TargetMod);
        assert!(snapshot.gaps().is_empty());
    }

    #[test]
    fn a_fixture_and_a_live_tree_of_identical_content_agree_on_the_fingerprint() {
        // The property that makes fixtures worth having: a golden test's snapshot is the
        // same identity a real build would produce, so a fixture cannot quietly exercise a
        // different scheme.
        fn write(root: &Path, relative: &str, contents: &[u8]) {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        let dir = TempDir::new().unwrap();
        write(dir.path(), "descriptor.mod", DESCRIPTOR);
        write(
            dir.path(),
            "common/technology/zz_oracle_tech.txt",
            TECHNOLOGY,
        );
        // Excluded by policy in both worlds, so its absence from the fixture is not a
        // difference between them.
        write(dir.path(), "gfx/models/ship.dds", b"binary-ish");

        let live = match crate::source::establish(SourceKind::TargetMod, dir.path()).unwrap() {
            crate::source::Established::Complete(source) => source,
            crate::source::Established::Incomplete(source) => {
                panic!("staged tree has gaps: {:?}", source.snapshot().gaps())
            }
        };
        assert_eq!(
            oracle_target().build().unwrap().fingerprint(),
            live.snapshot().fingerprint()
        );
    }

    #[test]
    fn a_seeded_asset_reads_back_and_an_unseeded_one_is_absent() {
        let snapshot = oracle_target()
            .with_asset("gfx/models/ship.dds", b"binary-ish")
            .build()
            .unwrap();

        let read = snapshot.read_asset(&path("gfx/models/ship.dds"));
        let AssetRead::Captured(captured) = read else {
            panic!("expected the seeded bytes, got {read:?}");
        };
        assert_eq!(captured.bytes.as_slice(), b"binary-ish");
        assert_eq!(captured.kind, SourceKind::TargetMod);
        assert_eq!(
            snapshot.read_asset(&path("gfx/models/absent.dds")),
            AssetRead::Absent(AssetAbsence::NotFound)
        );
        assert_eq!(
            snapshot
                .captured_assets()
                .iter()
                .map(|asset| asset.logical.as_str())
                .collect::<Vec<_>>(),
            vec!["gfx/models/ship.dds"]
        );
    }

    #[test]
    fn an_asset_read_of_enumerated_content_is_served_from_the_fixture_content() {
        let snapshot = oracle_target().build().unwrap();

        let read = snapshot.read_asset(&path("descriptor.mod"));

        let AssetRead::Captured(captured) = read else {
            panic!("enumerated content answers an asset read");
        };
        assert_eq!(captured.bytes.as_slice(), DESCRIPTOR);
    }

    #[test]
    fn an_asset_read_of_a_collided_path_refuses_to_pick_a_winner() {
        // The collision rule has to extend to asset reads, or it is not a refusal: without
        // the check, `observe_asset` falls through to the backing, and on a live tree that
        // means answering with whichever raw entry the NFC spelling lands on — the arbitrary
        // winner `enumerate` exists to refuse.
        //
        // What this asserts is `Collision` rather than `NotFound`: a memory backing has no
        // raw entries to pick between, so removing the check turns this into a plain miss.
        // The live half — which entry the fallthrough would have read — is the part macOS
        // cannot stage, since APFS cannot hold two names that normalize alike (D-111).
        let snapshot = oracle_target()
            .with_collision(
                "common/technology/t\u{e9}ch.txt",
                &[
                    "common/technology/te\u{301}ch.txt",
                    "common/technology/t\u{e9}ch.txt",
                ],
            )
            .build()
            .unwrap();

        assert_eq!(
            snapshot.read_asset(&path("common/technology/t\u{e9}ch.txt")),
            AssetRead::Absent(AssetAbsence::Collision)
        );
        assert!(snapshot.captured_assets().is_empty());
    }

    #[test]
    fn a_declared_collision_makes_the_fixture_incomplete_and_moves_its_fingerprint() {
        let clean = oracle_target().build().unwrap();
        let collided = oracle_target()
            .with_collision("common/technology/a.txt", &["common/technology/a.txt", "x"])
            .build()
            .unwrap();

        assert!(clean.gaps().is_empty());
        assert!(!collided.gaps().is_empty());
        assert_eq!(collided.gaps().collisions.len(), 1);
        // Same content, different observation, therefore different identity (D-112).
        assert_ne!(clean.fingerprint(), collided.fingerprint());
    }

    #[test]
    fn a_collision_needs_two_raw_spellings() {
        assert_eq!(
            oracle_target()
                .with_collision("common/technology/a.txt", &["common/technology/a.txt"])
                .build()
                .unwrap_err(),
            FixtureError::NotACollision {
                logical: path("common/technology/a.txt")
            }
        );
    }

    #[test]
    fn a_collision_cannot_shadow_a_file_in_either_declaration_order() {
        // Enumeration emits a surviving file or a collision for a path, never both. The
        // reversed order is the one that used to build `Ok`, because `with_collision`
        // consulted `self.files` but `with_file` never consulted `self.gaps` — and the
        // corpus it produced answered `read_asset` with `Captured` where a live snapshot
        // answers `Absent(Collision)`.
        let labels = ["descriptor.mod", "Descriptor.mod"];
        assert_eq!(
            oracle_target()
                .with_collision("descriptor.mod", &labels)
                .build()
                .unwrap_err(),
            FixtureError::Duplicate {
                logical: path("descriptor.mod")
            }
        );
        assert_eq!(
            FixtureCorpus::new(SourceKind::TargetMod)
                .with_collision("descriptor.mod", &labels)
                .with_file("descriptor.mod", DESCRIPTOR)
                .build()
                .unwrap_err(),
            FixtureError::Duplicate {
                logical: path("descriptor.mod")
            }
        );
    }

    #[test]
    fn a_collision_cannot_be_declared_twice() {
        // `classify_entries` keys collisions by logical path, so one path collides at most
        // once; two declarations would also encode as two items in the digest.
        let labels = ["common/technology/a.txt", "common/technology/A.txt"];
        assert_eq!(
            oracle_target()
                .with_collision("common/technology/a.txt", &labels)
                .with_collision("common/technology/a.txt", &labels)
                .build()
                .unwrap_err(),
            FixtureError::Duplicate {
                logical: path("common/technology/a.txt")
            }
        );
    }

    #[test]
    fn a_collision_at_a_policy_excluded_path_is_refused() {
        // The walk only ever considers paths the policy selects, so it cannot observe a
        // collision anywhere else — the same rule `with_file` applies to content.
        assert_eq!(
            oracle_target()
                .with_collision(
                    "sound/effects.txt",
                    &["sound/effects.txt", "sound/Effects.txt"]
                )
                .build()
                .unwrap_err(),
            FixtureError::FileExcludedByPolicy {
                logical: path("sound/effects.txt")
            }
        );
    }

    #[test]
    fn a_collided_path_never_reads_as_captured_in_any_declaration_order() {
        // The agreement property, stated as what is checkable here: a live snapshot answers
        // `Absent(Collision)` for a collided path, so no corpus may answer `Captured` for
        // one. Both halves hold — a corpus that pairs a collision with a file is unbuildable
        // (above), and one that does not never has content at that path to short-circuit to.
        //
        // The live half cannot be staged: APFS cannot hold two names that normalize alike
        // (D-111). What makes the two agree is that `observe_asset` is backing-agnostic —
        // the content shortcut, the collision refusal, and their order are one code path
        // that a fixture and a live snapshot both run.
        let collided = "common/technology/zz_oracle_tech.txt";
        let snapshot = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", DESCRIPTOR)
            .with_collision(
                collided,
                &[collided, "common/technology/ZZ_oracle_tech.txt"],
            )
            .build()
            .unwrap();

        assert_eq!(
            snapshot.read_asset(&path(collided)),
            AssetRead::Absent(AssetAbsence::Collision)
        );
        assert_eq!(snapshot.read(&path(collided)), None);
        assert!(snapshot.captured_assets().is_empty());
    }

    #[test]
    fn a_path_the_policy_excludes_is_refused_rather_than_ignored() {
        assert_eq!(
            oracle_target()
                .with_file("gfx/models/ship.dds", b"binary-ish")
                .build()
                .unwrap_err(),
            FixtureError::FileExcludedByPolicy {
                logical: path("gfx/models/ship.dds")
            }
        );
        assert_eq!(
            oracle_target()
                .with_file("sound/effects.txt", b"sounds\n")
                .build()
                .unwrap_err(),
            FixtureError::FileExcludedByPolicy {
                logical: path("sound/effects.txt")
            }
        );
    }

    #[test]
    fn an_asset_the_policy_selects_belongs_in_with_file() {
        assert_eq!(
            oracle_target()
                .with_asset("common/technology/02_tech.txt", b"tech_c = {}\n")
                .build()
                .unwrap_err(),
            FixtureError::AssetSelectedByPolicy {
                logical: path("common/technology/02_tech.txt")
            }
        );
    }

    #[test]
    fn a_path_that_is_not_a_logical_path_is_refused() {
        let refused = FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file("common/../escape.txt", b"x")
            .build()
            .unwrap_err();
        assert_eq!(
            refused,
            FixtureError::InvalidPath {
                entry: "common/../escape.txt".to_owned(),
                error: PathError::DotComponent,
            }
        );
        assert!(refused.to_string().contains("common/../escape.txt"));
    }

    #[test]
    fn a_repeated_path_is_refused_rather_than_overwritten() {
        assert_eq!(
            oracle_target()
                .with_file("descriptor.mod", b"name=\"Other\"\n")
                .build()
                .unwrap_err(),
            FixtureError::Duplicate {
                logical: path("descriptor.mod")
            }
        );
        assert_eq!(
            oracle_target()
                .with_asset("gfx/models/ship.dds", b"one")
                .with_asset("gfx/models/ship.dds", b"two")
                .build()
                .unwrap_err(),
            FixtureError::Duplicate {
                logical: path("gfx/models/ship.dds")
            }
        );
    }

    #[test]
    fn the_first_refusal_is_the_one_reported() {
        // A refused entry never reaches a map — `with_file` refuses *instead of* inserting —
        // so neither of these two is in the corpus. What the test pins is which one is
        // named: the entry the author has to fix, not the last one seen.
        assert_eq!(
            oracle_target()
                .with_file("sound/first.txt", b"x")
                .with_file("licenses/second.txt", b"y")
                .build()
                .unwrap_err(),
            FixtureError::FileExcludedByPolicy {
                logical: path("sound/first.txt")
            }
        );
    }

    #[test]
    fn fixture_errors_render_for_a_test_failure() {
        let errors = [
            FixtureError::InvalidPath {
                entry: "a\\b".to_owned(),
                error: PathError::BackslashComponent,
            },
            FixtureError::FileExcludedByPolicy {
                logical: path("sound/a.txt"),
            },
            FixtureError::AssetSelectedByPolicy {
                logical: path("common/a.txt"),
            },
            FixtureError::Duplicate {
                logical: path("common/a.txt"),
            },
            FixtureError::NotACollision {
                logical: path("common/a.txt"),
            },
        ];
        for error in errors {
            let rendered = error.to_string();
            assert!(!rendered.is_empty());
            assert!(!rendered.contains("FixtureError"), "{rendered}");
        }
    }
}
