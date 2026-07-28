//! Semantic stream construction: the order a content family's surviving files are read in.
//!
//! This is where the resolver's central finding lives. **Two contributors are not two
//! ordered layers.** Script and sprite content is enumerated in one global normalized
//! logical-path order across Vanilla and the Target Mod, with no rank for either
//! (`docs/spikes/resolver-evaluation.md`, "There is no layer precedence. There is one path
//! order."). `r10-loadorder` proved it by applying one early-sorting mod filename to both a
//! first-wins and a last-wins registry: the mod won the first-wins one and lost the
//! last-wins one, which no layer model can produce.
//!
//! Localization is the documented exception, and it is an exception about *ordering*, not
//! about layers being real elsewhere: its stream is Vanilla files, then mod files in
//! `enabled_mods` order, then every `replace/` file, LIOS throughout (`r13`, `r14`, `r15`).
//! Filename order, which decides script registries, plays no part in it.
//!
//! Membership and ordering are separate questions on purpose. A [`FileScope`] says which
//! surviving files a registry draws from; a [`ContentFamily`] says what order they are read
//! in. Folding the two together would make "sprites sort like script" a fact restated at
//! every sprite row instead of decided once here.

use crate::canonical::path::LogicalPath;
use crate::source::SourceKind;

use super::selection::FileSelection;

/// The `localisation/<language>/replace/` component. Not a source and not a layer: a
/// directory whose files load last within the localization stream, from any position in
/// mod order (`r15-loc-modvmod`).
const LOCALIZATION_REPLACE_COMPONENT: &str = "replace";

/// A content family, which is to say a stream-ordering rule and the rows that share it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum ContentFamily {
    Script,
    Sprite,
    Localization,
}

/// How a family orders its surviving files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StreamOrder {
    /// One global normalized logical-path order across both contributors.
    GlobalPath,
    /// Vanilla, then mod files in enabled-mod order, then every `replace/` file.
    LocalizationLayered,
}

impl ContentFamily {
    /// The distinction decided once, so no row and no consumer re-derives it.
    pub fn order(self) -> StreamOrder {
        match self {
            // Sprites were measured separately from script registries (`r17`, `r18`) and
            // landed on the same answer. They stay a distinct family so that evidence
            // separating them later changes this arm rather than every sprite row.
            Self::Script | Self::Sprite => StreamOrder::GlobalPath,
            Self::Localization => StreamOrder::LocalizationLayered,
        }
    }
}

/// Which surviving files a registry draws from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileScope {
    /// A logical directory prefix, matched on whole components.
    pub directory: &'static str,
    pub extensions: &'static [&'static str],
    /// Whether files in nested subdirectories are in scope. Declared rather than assumed:
    /// `common/inline_scripts` nests (`r11-inline`) and `common/technology` was measured
    /// flat, so a single answer here would be a guess for one of them.
    pub recursive: bool,
}

impl FileScope {
    pub fn admits(&self, logical: &LogicalPath) -> bool {
        let Some(rest) = logical
            .as_str()
            .strip_prefix(self.directory)
            .and_then(|rest| rest.strip_prefix('/'))
        else {
            return false;
        };
        if !self.recursive && rest.contains('/') {
            return false;
        }
        match rest.rsplit_once('.') {
            Some((_, extension)) => self.extensions.contains(&extension),
            None => false,
        }
    }
}

/// One file's place in a family's semantic stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StreamEntry {
    /// Position in the stream, from zero. The value provenance records as resolution order.
    pub order: u32,
    pub source: SourceKind,
    pub logical: LogicalPath,
}

/// The surviving files of `scope`, in `family`'s order.
pub(super) fn build(
    selection: &FileSelection,
    family: ContentFamily,
    scope: FileScope,
) -> Vec<StreamEntry> {
    // `FileSelection::surviving` yields logical-path order already, which is what
    // `GlobalPath` wants; the layered order is a stable partition of that same sequence, so
    // both cases keep path order inside whatever segment they land in.
    let admitted = selection
        .surviving()
        .filter(|(logical, _)| scope.admits(logical));

    let ordered: Vec<(&LogicalPath, SourceKind)> = match family.order() {
        StreamOrder::GlobalPath => admitted.collect(),
        StreamOrder::LocalizationLayered => {
            let mut segments: [Vec<(&LogicalPath, SourceKind)>; 3] =
                [Vec::new(), Vec::new(), Vec::new()];
            for (logical, source) in admitted {
                segments[localization_segment(logical, source)].push((logical, source));
            }
            segments.into_iter().flatten().collect()
        }
    };

    ordered
        .into_iter()
        .enumerate()
        .map(|(order, (logical, source))| StreamEntry {
            order: order as u32,
            source,
            logical: logical.clone(),
        })
        .collect()
}

/// Which of the three localization segments a file belongs to.
///
/// The middle segment is "mod files in `enabled_mods` order", which in the MVP's
/// two-contributor scope is one mod — so within it, and within the other two, files keep
/// normalized logical-path order. That is the deterministic choice rather than a measured
/// one: no oracle record distinguishes two ordinary files of one source, and reading the
/// filesystem's order instead would make the answer depend on the host.
fn localization_segment(logical: &LogicalPath, source: SourceKind) -> usize {
    if logical
        .components()
        .any(|component| component == LOCALIZATION_REPLACE_COMPONENT)
    {
        return 2;
    }
    match source {
        SourceKind::VanillaContent => 0,
        SourceKind::TargetMod => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::fixture::FixtureCorpus;
    use crate::source::snapshot::SourceSnapshot;

    const SCRIPT: FileScope = FileScope {
        directory: "common/technology",
        extensions: &["txt"],
        recursive: false,
    };

    const LOCALIZATION: FileScope = FileScope {
        directory: "localisation",
        extensions: &["yml"],
        recursive: true,
    };

    fn corpus(kind: SourceKind, files: &[&str]) -> SourceSnapshot {
        files
            .iter()
            .fold(FixtureCorpus::new(kind), |corpus, logical| {
                corpus.with_file(logical, b"x = {}")
            })
            .build()
            .expect("a well-formed fixture corpus")
    }

    fn stream(
        vanilla: &[&str],
        target: &[&str],
        family: ContentFamily,
        scope: FileScope,
    ) -> Vec<(String, SourceKind)> {
        let selection = super::super::selection::select(
            &corpus(SourceKind::VanillaContent, vanilla),
            &corpus(SourceKind::TargetMod, target),
        );
        build(&selection, family, scope)
            .into_iter()
            .map(|entry| (entry.logical.as_str().to_owned(), entry.source))
            .collect()
    }

    #[test]
    fn script_interleaves_both_sources_by_path_with_no_rank() {
        // The r10 shape at the stream level: an early-sorting mod file sits *before* a
        // vanilla file, which is the only reason a first-wins registry can be overridden and
        // a last-wins one cannot. Any layer model puts both mod files at one end.
        assert_eq!(
            stream(
                &[
                    "common/technology/00_synthetic_dawn_tech.txt",
                    "common/technology/50_mid.txt",
                ],
                &[
                    "common/technology/!!!_oracle_tech.txt",
                    "common/technology/zz_late.txt",
                ],
                ContentFamily::Script,
                SCRIPT,
            ),
            vec![
                (
                    "common/technology/!!!_oracle_tech.txt".to_owned(),
                    SourceKind::TargetMod
                ),
                (
                    "common/technology/00_synthetic_dawn_tech.txt".to_owned(),
                    SourceKind::VanillaContent
                ),
                (
                    "common/technology/50_mid.txt".to_owned(),
                    SourceKind::VanillaContent
                ),
                (
                    "common/technology/zz_late.txt".to_owned(),
                    SourceKind::TargetMod
                ),
            ]
        );
    }

    #[test]
    fn sprites_order_exactly_as_script_does() {
        let scope = FileScope {
            directory: "interface",
            extensions: &["gfx"],
            recursive: false,
        };
        let by_family = |family| {
            stream(
                &["interface/50_vanilla.gfx"],
                &["interface/00_oracle_sprites.gfx"],
                family,
                scope,
            )
        };
        assert_eq!(
            by_family(ContentFamily::Sprite),
            by_family(ContentFamily::Script)
        );
        assert_eq!(
            by_family(ContentFamily::Sprite)[0].0,
            "interface/00_oracle_sprites.gfx"
        );
    }

    #[test]
    fn localization_orders_vanilla_then_mod_then_replace_and_ignores_filename_order() {
        // `00_oracle…` sorts before every vanilla name here, and still loads after them:
        // filename order decides script registries and plays no part in localization.
        // `replace/` comes last from an early-sorting name, which is the `r15` result.
        assert_eq!(
            stream(
                &[
                    "localisation/english/technology_l_english.yml",
                    "localisation/english/zz_vanilla_l_english.yml",
                ],
                &[
                    "localisation/english/00_oracle_l_english.yml",
                    "localisation/english/replace/00_oracle_replace_l_english.yml",
                ],
                ContentFamily::Localization,
                LOCALIZATION,
            )
            .into_iter()
            .map(|(logical, _)| logical)
            .collect::<Vec<_>>(),
            vec![
                "localisation/english/technology_l_english.yml".to_owned(),
                "localisation/english/zz_vanilla_l_english.yml".to_owned(),
                "localisation/english/00_oracle_l_english.yml".to_owned(),
                "localisation/english/replace/00_oracle_replace_l_english.yml".to_owned(),
            ]
        );
    }

    #[test]
    fn a_scope_admits_only_its_directory_extensions_and_declared_depth() {
        let logical = |raw: &str| LogicalPath::parse(raw).expect("a test path");
        assert!(SCRIPT.admits(&logical("common/technology/00_a.txt")));
        assert!(!SCRIPT.admits(&logical("common/technology/00_a.gfx")));
        assert!(!SCRIPT.admits(&logical("common/buildings/00_a.txt")));
        // The prefix trap again, one level up from `selection::covers`.
        assert!(!SCRIPT.admits(&logical("common/technology_extra/00_a.txt")));
        assert!(!SCRIPT.admits(&logical("common/technology/nested/00_a.txt")));
        let recursive = FileScope {
            recursive: true,
            ..SCRIPT
        };
        assert!(recursive.admits(&logical("common/technology/nested/00_a.txt")));
    }

    #[test]
    fn stream_positions_are_contiguous_from_zero() {
        let selection = super::super::selection::select(
            &corpus(SourceKind::VanillaContent, &["common/technology/a.txt"]),
            &corpus(SourceKind::TargetMod, &["common/technology/b.txt"]),
        );
        let orders: Vec<u32> = build(&selection, ContentFamily::Script, SCRIPT)
            .iter()
            .map(|entry| entry.order)
            .collect();
        assert_eq!(orders, vec![0, 1]);
    }
}
