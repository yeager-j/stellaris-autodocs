//! Common file selection: which files survive to contribute at all, before any registry
//! looks at a definition.
//!
//! Two mechanisms, and **this is the only stage where source origin decides anything**
//! (`docs/spikes/resolver-evaluation.md`, "There is no layer precedence. There is one path
//! order."):
//!
//! 1. **Directory replacement.** A Target Mod `replace_path` declaration excludes every
//!    *other* source's files at or under that directory. The declaring mod's own files there
//!    still load — `r3-replace-path` registered all three of its new technologies while
//!    7,708 vanilla references failed. So the rule is "exclude every other source's files in
//!    this directory", not "this directory now holds only what I ship".
//! 2. **Exact path collision.** Two sources at one normalized logical path: the Target Mod's
//!    file replaces Vanilla's entirely, and the loser contributes nothing — *including keys
//!    the winner never mentions* (`r6-pathcollision`: the mod's file at
//!    `common/technology/00_astral_planes_tech.txt` defined one sentinel, and both vanilla
//!    technologies at that path vanished, breaking 61 references). Merge-by-key is ruled
//!    out. Layer decides here only because identical paths cannot be ordered against each
//!    other.
//!
//! Exclusion runs first, so a Vanilla file inside a replaced directory is reported as
//! *excluded* rather than as shadowed by whatever the mod happens to ship at that path. The
//! surviving set is the same either way; the reason a reader is given is not, and a
//! provenance record that misattributed the cause would be worse than a coarse one.

use crate::canonical::path::LogicalPath;
use crate::source::{SourceKind, SourceSnapshot};
use std::collections::BTreeMap;

use super::resolved::Removal;

use super::super::parser::{self, SourceIdentity, Value};

/// The root descriptor, the only `.mod` file a source's own `replace_path` can come from
/// (`source::policy`, `DESCRIPTOR_EXTENSION`).
const DESCRIPTOR: &str = "descriptor.mod";

const REPLACE_PATH: &str = "replace_path";

/// One file that will contribute nothing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct RemovedFile {
    pub source: SourceKind,
    pub logical: LogicalPath,
    pub removal: Removal,
}

/// The outcome of common file selection, shared by every content family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FileSelection {
    /// Every surviving file and the source that supplies it, in logical-path order.
    surviving: BTreeMap<LogicalPath, SourceKind>,
    removed: Vec<RemovedFile>,
    /// The Target Mod's declarations, normalized, in the order the descriptor states them.
    /// Kept because "which declaration excluded this file" is a provenance question.
    declarations: Vec<LogicalPath>,
    /// Declared `replace_path` values that are not usable logical paths. Retained rather
    /// than dropped: a mod whose declaration cannot be honoured is a visible fact, not a
    /// silent no-op.
    unusable_declarations: Vec<String>,
}

impl FileSelection {
    pub fn surviving(&self) -> impl Iterator<Item = (&LogicalPath, SourceKind)> {
        self.surviving
            .iter()
            .map(|(logical, source)| (logical, *source))
    }

    pub fn source_of(&self, logical: &LogicalPath) -> Option<SourceKind> {
        self.surviving.get(logical).copied()
    }

    pub fn removed(&self) -> &[RemovedFile] {
        &self.removed
    }

    pub fn declarations(&self) -> &[LogicalPath] {
        &self.declarations
    }

    pub fn unusable_declarations(&self) -> &[String] {
        &self.unusable_declarations
    }
}

/// Selects across the MVP's two contributors.
///
/// Vanilla Content declares no `replace_path`: it is not a mod and ships no descriptor of
/// its own. Reading declarations from the Target Mod alone is therefore a statement about
/// the two-contributor scope, not a shortcut — a third contributor would make this a fold
/// over sources rather than a pair.
pub(super) fn select(vanilla: &SourceSnapshot, target: &SourceSnapshot) -> FileSelection {
    let (declarations, unusable_declarations) = replace_paths(target);
    let mut surviving = BTreeMap::new();
    let mut removed = Vec::new();

    for logical in vanilla.paths() {
        match declarations
            .iter()
            .find(|directory| covers(directory, logical))
        {
            Some(directory) => removed.push(RemovedFile {
                source: SourceKind::VanillaContent,
                logical: logical.clone(),
                removal: Removal::ReplacedDirectory {
                    declaration: directory.clone(),
                },
            }),
            None => {
                surviving.insert(logical.clone(), SourceKind::VanillaContent);
            }
        }
    }

    for logical in target.paths() {
        if let Some(displaced) = surviving.insert(logical.clone(), SourceKind::TargetMod) {
            debug_assert_eq!(displaced, SourceKind::VanillaContent);
            removed.push(RemovedFile {
                source: displaced,
                logical: logical.clone(),
                removal: Removal::ShadowedByPathCollision {
                    winner: SourceKind::TargetMod,
                },
            });
        }
    }

    removed.sort();
    FileSelection {
        surviving,
        removed,
        declarations,
        unusable_declarations,
    }
}

/// Whether `directory` replaces `logical` — matched on whole components, so
/// `common/tech` does not swallow `common/technology`.
///
/// A declaration naming a file rather than a directory would only ever match that exact
/// path, which is the same answer an exact-path collision gives; no oracle record exercises
/// one, and nothing here needs to distinguish the two cases.
fn covers(directory: &LogicalPath, logical: &LogicalPath) -> bool {
    let mut declared = directory.components();
    let mut candidate = logical.components();
    loop {
        match (declared.next(), candidate.next()) {
            (None, Some(_)) => return true,
            (None, None) => return true,
            (Some(_), None) => return false,
            (Some(left), Some(right)) if left == right => {}
            (Some(_), Some(_)) => return false,
        }
    }
}

/// Every `replace_path` the Target Mod's root descriptor declares, in declaration order.
///
/// Parsed with the real parser rather than read through `discovery::descriptor`, which is
/// advisory display metadata for the Mod Library and says so: "analysis never consumes this
/// reader". A `replace_path` decides which files exist for the whole build, so it is not
/// advisory, and a tolerant line scanner is not the authority for it.
///
/// An absent or unreadable descriptor means no declarations. That is the same outcome as a
/// descriptor that declares none, and it is correct: `replace_path` is opt-in, so its
/// absence cannot be distinguished from a mod that never wanted it.
fn replace_paths(target: &SourceSnapshot) -> (Vec<LogicalPath>, Vec<String>) {
    let Ok(descriptor) = LogicalPath::parse(DESCRIPTOR) else {
        unreachable!("a constant literal relative path parses");
    };
    let Some(bytes) = target.read(&descriptor) else {
        return (Vec::new(), Vec::new());
    };

    let identity = SourceIdentity::new(target.kind(), descriptor);
    let parsed = parser::parse(identity, bytes.as_slice());

    let mut declared = Vec::new();
    let mut unusable = Vec::new();
    for (field, _) in parsed.definitions() {
        if field.key.text() != REPLACE_PATH {
            continue;
        }
        let Value::Scalar(scalar) = &field.value else {
            unusable.push(format!("{REPLACE_PATH} is not a single value"));
            continue;
        };
        let raw = scalar.text();
        match LogicalPath::parse(raw.as_ref()) {
            Ok(logical) if !declared.contains(&logical) => declared.push(logical),
            Ok(_) => {}
            Err(error) => unusable.push(format!("{REPLACE_PATH}=\"{raw}\": {error}")),
        }
    }
    (declared, unusable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::fixture::FixtureCorpus;

    fn vanilla(files: &[(&str, &str)]) -> SourceSnapshot {
        corpus(SourceKind::VanillaContent, files)
    }

    fn target(files: &[(&str, &str)]) -> SourceSnapshot {
        corpus(SourceKind::TargetMod, files)
    }

    fn corpus(kind: SourceKind, files: &[(&str, &str)]) -> SourceSnapshot {
        files
            .iter()
            .fold(FixtureCorpus::new(kind), |corpus, (logical, body)| {
                corpus.with_file(logical, body.as_bytes())
            })
            .build()
            .expect("a well-formed fixture corpus")
    }

    fn survivors(selection: &FileSelection) -> Vec<(String, SourceKind)> {
        selection
            .surviving()
            .map(|(logical, source)| (logical.as_str().to_owned(), source))
            .collect()
    }

    #[test]
    fn distinct_paths_from_both_sources_all_survive() {
        let selection = select(
            &vanilla(&[("common/technology/00_vanilla.txt", "a = {}")]),
            &target(&[
                ("descriptor.mod", "name=\"m\""),
                ("common/technology/zz_mod.txt", "b = {}"),
            ]),
        );
        assert_eq!(
            survivors(&selection),
            vec![
                (
                    "common/technology/00_vanilla.txt".to_owned(),
                    SourceKind::VanillaContent
                ),
                (
                    "common/technology/zz_mod.txt".to_owned(),
                    SourceKind::TargetMod
                ),
                ("descriptor.mod".to_owned(), SourceKind::TargetMod),
            ]
        );
        assert!(selection.removed().is_empty());
    }

    #[test]
    fn an_exact_path_collision_removes_the_vanilla_file_whole() {
        // r6. The mod's file mentions neither key vanilla defined at that path, and the
        // assertion that matters is that both are gone: a merge-by-key implementation
        // passes every other check here.
        let path = "common/technology/00_astral_planes_tech.txt";
        let selection = select(
            &vanilla(&[(path, "tech_one = {}\ntech_two = {}")]),
            &target(&[
                ("descriptor.mod", "name=\"m\""),
                (path, "tech_sentinel = {}"),
            ]),
        );
        assert_eq!(
            selection.source_of(&logical(path)),
            Some(SourceKind::TargetMod)
        );
        assert_eq!(
            selection.removed(),
            [RemovedFile {
                source: SourceKind::VanillaContent,
                logical: logical(path),
                removal: Removal::ShadowedByPathCollision {
                    winner: SourceKind::TargetMod
                },
            }]
        );
    }

    #[test]
    fn replace_path_excludes_other_sources_and_keeps_the_declarer_s_own() {
        // r3. Both halves in one test, because either alone is satisfied by a wrong rule:
        // "exclude the directory entirely" passes the exclusion half, and "do nothing"
        // passes the retention half.
        let selection = select(
            &vanilla(&[
                ("common/technology/00_vanilla.txt", "a = {}"),
                ("common/buildings/00_vanilla.txt", "b = {}"),
            ]),
            &target(&[
                (
                    "descriptor.mod",
                    "name=\"m\"\nreplace_path=\"common/technology\"",
                ),
                ("common/technology/zz_mod.txt", "c = {}"),
            ]),
        );
        assert_eq!(
            survivors(&selection),
            vec![
                (
                    "common/buildings/00_vanilla.txt".to_owned(),
                    SourceKind::VanillaContent
                ),
                (
                    "common/technology/zz_mod.txt".to_owned(),
                    SourceKind::TargetMod
                ),
                ("descriptor.mod".to_owned(), SourceKind::TargetMod),
            ]
        );
        assert_eq!(
            selection.removed(),
            [RemovedFile {
                source: SourceKind::VanillaContent,
                logical: logical("common/technology/00_vanilla.txt"),
                removal: Removal::ReplacedDirectory {
                    declaration: logical("common/technology")
                },
            }]
        );
    }

    #[test]
    fn a_declaration_matches_whole_components_only() {
        assert!(covers(
            &logical("common/tech"),
            &logical("common/tech/a.txt")
        ));
        assert!(covers(&logical("common/tech"), &logical("common/tech")));
        assert!(covers(
            &logical("common/tech"),
            &logical("common/tech/deep/a.txt")
        ));
        // The prefix trap: a string `starts_with` would exclude the whole technology tree
        // for a mod that only asked to replace `common/tech`.
        assert!(!covers(
            &logical("common/tech"),
            &logical("common/technology/a.txt")
        ));
        assert!(!covers(&logical("common/tech/a.txt"), &logical("common")));
    }

    #[test]
    fn declarations_are_read_from_the_descriptor_in_order_and_deduplicated() {
        let snapshot = target(&[(
            "descriptor.mod",
            "name=\"m\"\nreplace_path=\"common/buildings\"\nreplace_path=\"common/technology\"\n\
             replace_path=\"common/buildings\"\ntags={ \"Utilities\" }",
        )]);
        let (declared, unusable) = replace_paths(&snapshot);
        assert_eq!(
            declared,
            [logical("common/buildings"), logical("common/technology")]
        );
        assert!(unusable.is_empty());
    }

    #[test]
    fn an_unusable_declaration_is_recorded_rather_than_silently_dropped() {
        let selection = select(
            &vanilla(&[("common/technology/00_vanilla.txt", "a = {}")]),
            &target(&[("descriptor.mod", "name=\"m\"\nreplace_path=\"/common\"")]),
        );
        assert_eq!(
            selection.unusable_declarations(),
            ["replace_path=\"/common\": path is absolute or carries a drive prefix"]
        );
        // And nothing was excluded on the strength of a declaration nobody could honour.
        assert_eq!(
            selection.source_of(&logical("common/technology/00_vanilla.txt")),
            Some(SourceKind::VanillaContent)
        );
    }

    #[test]
    fn a_target_without_a_descriptor_declares_nothing() {
        let selection = select(
            &vanilla(&[("common/technology/00_vanilla.txt", "a = {}")]),
            &target(&[("common/technology/zz_mod.txt", "b = {}")]),
        );
        assert!(selection.declarations().is_empty());
        assert!(selection.removed().is_empty());
    }

    fn logical(raw: &str) -> LogicalPath {
        LogicalPath::parse(raw).expect("a test path")
    }
}
