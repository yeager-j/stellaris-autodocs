//! The build stage that turns resolved localization bytes into per-language effective tables.
//!
//! # Why this exists as its own thing
//!
//! The design has `analysis` invoke localization ingestion as an internal build stage
//! (docs/technical-design.md, "Localization module"). The resolver stops before `.yml`
//! interpretation on purpose, and `localization` must not reach back for a resolution, so
//! something has to sit between them. This is it, and it is deliberately the *only* thing
//! there: one function, one exhaustive translation, no policy.
//!
//! # The boundary it records
//!
//! `localization` declares the question — [`LocalizationInput`] names only crate-public
//! primitives — and `analysis` answers it here. Three alternatives were rejected (Phase 5A,
//! STE-37):
//!
//! - Widening the resolver's handoff types to the crate. That publishes the five-kind
//!   provenance vocabulary as a cross-module contract to buy one struct, and closes an edge
//!   against the one sanctioned direction.
//! - Re-homing the handoff type into `localization`. It either keeps [`FactProvenance`], which
//!   is not visible outside `analysis`, or drops it — in which case it is [`LocalizationInput`]
//!   under another name but built inside the resolver, giving the resolver two output
//!   vocabularies and a `crate::localization` import it has no business holding.
//! - Letting `localization` call the resolver. Inverts the sanctioned direction outright.
//!
//! The translation is not a wrapper that only delegates. It makes one decision — which of the
//! resolver's file-selection facts a key-level table may attribute, and in what vocabulary —
//! and [`removed_file`] states it as an exhaustive `match`, so a removal mechanism added to the
//! resolver fails to compile here before it can silently arrive downstream as something else.
//!
//! Both halves of the boundary are gated: `localization` may not name an `analysis` type
//! (`localization::tests::localization_names_no_analysis_type`), and `analysis` may not build
//! these tables anywhere but here ([`only_the_stage_builds_localization_tables`]). This half
//! was broken by hand once — a `crate::localization::ingest` reference seeded into
//! `analysis::version` — and observed failing, then restored. A seeded *comment* correctly did
//! not trip it.
//!
//! [`FactProvenance`]: super::resolver::FactProvenance
//! [`only_the_stage_builds_localization_tables`]: tests::only_the_stage_builds_localization_tables

#[cfg(test)]
mod census;

use crate::localization::{self, EffectiveTables, LocalizationInput, RemovedFile, StreamedFile};

use super::resolver::{FactSite, Refusal, Removal, Resolution, ShadowedLocalizationFile};

/// Ingest one build's localization.
///
/// The only failure is the resolver's own — an unusable `replace_path` declaration refuses
/// file selection before any bytes exist — so it passes through unwrapped rather than being
/// re-worded into a stage-level union over one variant. Everything localization itself
/// declines to interpret is a typed condition on the tables, not an error here.
pub(in crate::analysis) fn tables(resolution: &Resolution) -> Result<EffectiveTables, Refusal> {
    let stream = resolution.localization_files()?;
    let streamed = stream
        .files
        .into_iter()
        .enumerate()
        .map(|(index, file)| {
            // Position in the vector is what ingestion reads as stream order. The resolver's
            // own ordinal is asserted against it here rather than carried onward: two
            // authorities on one file's position is exactly one too many, and a stream that
            // disagreed with itself would be a resolver defect rather than a localization
            // input.
            debug_assert_eq!(
                file.order as usize, index,
                "the localization stream is not densely ordered from zero"
            );
            StreamedFile {
                source: file.source,
                logical: file.logical,
                bytes: file.bytes,
            }
        })
        .collect();
    let removed = stream
        .shadowed_files
        .into_iter()
        .map(removed_file)
        .collect();

    Ok(localization::ingest(LocalizationInput {
        streamed,
        removed,
    }))
}

/// Restate one whole-file selection loss in localization's vocabulary.
fn removed_file(file: ShadowedLocalizationFile) -> RemovedFile {
    let FactSite::RemovedBySelection {
        source,
        logical,
        removal,
    } = file.provenance.site
    else {
        unreachable!("the file-shadow projection produced a non-file fact");
    };
    let loss = match removal {
        Removal::ReplacedDirectory { declaration } => {
            localization::FileLoss::ReplacedDirectory { declaration }
        }
        Removal::ShadowedByPathCollision { winner } => {
            localization::FileLoss::ShadowedByPathCollision { winner }
        }
    };
    RemovedFile {
        source,
        logical,
        bytes: file.bytes,
        loss,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::localization::{FileLoss, FileOrigin, LanguageTag};
    use crate::source::SourceKind;
    use crate::source::fixture::FixtureCorpus;

    use crate::analysis::resolver::resolve;

    const STAGE: &str = "localization_stage";

    fn english() -> LanguageTag {
        LanguageTag::parse("l_english").expect("a well-formed language token")
    }

    fn corpus(kind: SourceKind, files: &[(&str, &[u8])]) -> crate::source::SourceSnapshot {
        let mut corpus = FixtureCorpus::new(kind);
        for (path, bytes) in files {
            corpus = corpus.with_file(path, bytes);
        }
        corpus.build().expect("a fixture corpus")
    }

    #[test]
    fn the_stage_carries_stream_order_into_the_tables() {
        let vanilla = corpus(
            SourceKind::VanillaContent,
            &[(
                "localisation/english/00_vanilla_l_english.yml",
                b"l_english:\n k:0 \"Vanilla\"\n",
            )],
        );
        // An early-sorting mod filename: under the script family's global path order it would
        // be read first and lose, so a winner of "Mod" is the localization stream's answer and
        // not an accident of enumeration.
        let target = corpus(
            SourceKind::TargetMod,
            &[
                ("descriptor.mod", b"name=\"stage\""),
                (
                    "localisation/english/!!!_mod_l_english.yml",
                    b"l_english:\n k:0 \"Mod\"\n",
                ),
            ],
        );

        let tables = tables(&resolve(&vanilla, &target)).expect("the stage resolves");
        let entry = tables.get(&english(), "k").expect("an entry");
        assert_eq!(entry.winner().value.as_str(), "Mod");
        assert!(matches!(
            tables.file(entry.winner().file).origin,
            FileOrigin::Streamed { order: 1 }
        ));
        assert!(matches!(
            tables.file(entry.shadowed()[0].file).origin,
            FileOrigin::Streamed { order: 0 }
        ));
    }

    #[test]
    fn a_path_collision_arrives_as_a_localization_file_loss() {
        // The one translation this stage performs, read end to end: the resolver's removal
        // reason reaches a key-level casualty intact.
        let vanilla = corpus(
            SourceKind::VanillaContent,
            &[(
                "localisation/english/collided_l_english.yml",
                b"l_english:\n kept:0 \"Vanilla\"\n lost:0 \"Lost\"\n",
            )],
        );
        let target = corpus(
            SourceKind::TargetMod,
            &[
                ("descriptor.mod", b"name=\"stage\""),
                (
                    "localisation/english/collided_l_english.yml",
                    b"l_english:\n kept:0 \"Mod\"\n",
                ),
            ],
        );

        let tables = tables(&resolve(&vanilla, &target)).expect("the stage resolves");
        let grouped = tables.casualties_by_file();
        let (index, by_language) = grouped.into_iter().next().expect("one shadowed file");
        assert_eq!(by_language[&english()], BTreeSet::from(["lost"]));
        assert_eq!(
            tables.file(index).origin,
            FileOrigin::Removed {
                loss: FileLoss::ShadowedByPathCollision {
                    winner: SourceKind::TargetMod
                }
            }
        );
        assert_eq!(tables.file(index).source, SourceKind::VanillaContent);
    }

    #[test]
    fn an_unreadable_replace_path_declaration_refuses_before_any_bytes_are_read() {
        let vanilla = corpus(SourceKind::VanillaContent, &[]);
        let target = corpus(
            SourceKind::TargetMod,
            &[("descriptor.mod", b"name=\"stage\"\nreplace_path=\"..\"")],
        );
        assert!(matches!(
            tables(&resolve(&vanilla, &target)),
            Err(Refusal::UnusableReplacePath { .. })
        ));
    }

    #[test]
    fn uninterpretable_bytes_become_a_condition_rather_than_a_refusal() {
        let vanilla = corpus(SourceKind::VanillaContent, &[]);
        let target = corpus(
            SourceKind::TargetMod,
            &[
                ("descriptor.mod", b"name=\"stage\""),
                (
                    "localisation/english/not_yaml_l_english.yml",
                    b"\xff\0not Clausewitz and not YAML",
                ),
            ],
        );

        let tables = tables(&resolve(&vanilla, &target)).expect("a build is not refused");
        assert_eq!(tables.conditions().len(), 1);
        assert_eq!(tables.conditions()[0].kind.name(), "NotUtf8");
        assert_eq!(tables.available().count(), 0);
    }

    /// Every line of `source` that builds the localization tables, outside a comment.
    fn ingest_calls(source: &str) -> Vec<String> {
        source
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim_start().starts_with("//"))
            .filter(|(_, line)| line.contains("localization::ingest"))
            .map(|(number, _)| format!("line {}", number + 1))
            .collect()
    }

    fn analysis_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("analysis")
    }

    /// Every module `analysis` declares under `#[cfg(test)]`, by name.
    ///
    /// Read from the declarations rather than listed here, so a module added on either side is
    /// classified by the same `#[cfg(test)]` the compiler reads. Whole test-only modules —
    /// `oracle`, `trial`, `conformance` — cannot be scoped by truncating at their test marker,
    /// because they have none: the marker is on their declaration.
    fn test_only_modules() -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        let mut stack = vec![analysis_root()];
        while let Some(directory) = stack.pop() {
            for entry in fs::read_dir(&directory).expect("an analysis directory is readable") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|extension| extension != "rs") {
                    continue;
                }
                let source = fs::read_to_string(&path).expect("a readable source file");
                let lines: Vec<_> = source.lines().map(str::trim).collect();
                for pair in lines.windows(2) {
                    if pair[0] == "#[cfg(test)]"
                        && let Some(name) = pair[1].strip_prefix("mod ")
                        && let Some(name) = name.strip_suffix(';')
                    {
                        names.insert(name.to_owned());
                    }
                }
            }
        }
        names
    }

    /// Every `.rs` file of shipped `analysis` code outside this stage.
    ///
    /// Shipped only, and for a reason specific to this direction: the r15 key-level
    /// expectation deliberately builds an input production cannot produce, since production
    /// supplies one Target Mod rank and the record measured two. Gating test code would forbid
    /// the one honest way to check that half of the record.
    fn analysis_sources_outside_the_stage() -> Vec<(PathBuf, String)> {
        fn walk(directory: &Path, skip: &BTreeSet<String>, found: &mut Vec<(PathBuf, String)>) {
            for entry in fs::read_dir(directory).expect("an analysis directory is readable") {
                let path = entry.expect("a directory entry").path();
                let stem = path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if skip.contains(&stem) {
                    continue;
                }
                if path.is_dir() {
                    walk(&path, skip, found);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    let whole = fs::read_to_string(&path).expect("a readable source file");
                    let shipped = whole
                        .split_once("#[cfg(test)]")
                        .map_or(whole.as_str(), |(shipped, _)| shipped)
                        .to_owned();
                    found.push((path, shipped));
                }
            }
        }
        let mut skip = test_only_modules();
        skip.insert(STAGE.to_owned());
        let mut found = Vec::new();
        walk(&analysis_root(), &skip, &mut found);
        found
    }

    /// The converse of localization's own boundary gate.
    ///
    /// `crate::localization` is `pub`, so nothing stops another part of `analysis` from
    /// building its own input and ingesting a second time. What must stay singular is the act
    /// of *building* the tables in shipped code — a second call with a differently assembled
    /// input is a second authority on what the mod says. The types are deliberately not gated:
    /// the Phase 6 documentation generator must be free to read `EffectiveTables` and
    /// `Language`.
    #[test]
    fn only_the_stage_builds_localization_tables() {
        let sources = analysis_sources_outside_the_stage();
        assert!(
            sources.len() >= 10,
            "the analysis walk found {} files, so the scan may have covered nothing",
            sources.len()
        );
        for (path, source) in sources {
            let found = ingest_calls(&source);
            assert!(found.is_empty(), "{}: {}", path.display(), found.join(", "));
        }
    }

    #[test]
    fn the_scan_detects_a_seeded_second_ingest_call() {
        let seeded = concat!(
            "fn generate() {\n",
            "    // localization::ingest in a comment does not count\n",
            "    let tables = localization::ingest(input);\n",
            "}\n"
        );
        assert_eq!(ingest_calls(seeded), ["line 3"]);
    }

    #[test]
    fn the_walk_reaches_the_resolver_and_skips_the_stage_and_the_test_modules() {
        // A walk that silently stopped at the top level would leave the whole resolver
        // unscanned while still passing; one that failed to skip this directory would report
        // the stage's own call; and one whose `#[cfg(test)]` reader broke would report the
        // oracle suite's deliberate second ingest.
        assert!(
            test_only_modules().contains("oracle"),
            "the test-only reader found {:?}, which does not include the oracle suite",
            test_only_modules()
        );
        let paths: Vec<_> = analysis_sources_outside_the_stage()
            .into_iter()
            .map(|(path, _)| path.to_string_lossy().replace('\\', "/"))
            .collect();
        assert!(paths.iter().any(|path| path.ends_with("resolver/mod.rs")));
        assert!(
            paths
                .iter()
                .any(|path| path.ends_with("resolver/stream.rs"))
        );
        assert!(!paths.iter().any(|path| path.contains(STAGE)));
        assert!(!paths.iter().any(|path| path.contains("/oracle/")));
    }
}
