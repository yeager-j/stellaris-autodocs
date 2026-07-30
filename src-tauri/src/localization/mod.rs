//! Owns the Stellaris localization language: ingestion, markup tokenization, fallback,
//! Static Localization Reference resolution, plain-text projection, and display tokens
//! (docs/technical-design.md, "Localization module"), plus detection of the configured game
//! language and the effective-language derivation.
//!
//! # The contract
//!
//! [`ingest`] takes every localization file of one build — the surviving ones in the order the
//! game reads them, and the ones removed before the stream with the bytes they would have
//! supplied — and answers with [`EffectiveTables`]: one table per language, each key carrying
//! the value the game uses, every value that lost, and the file and line each came from. Keys
//! that exist only in a removed file are enumerated separately as raw-key casualties, because
//! the game renders them as their own identifiers and Player Documentation must never present
//! one as a name.
//!
//! There is no failure outcome. Every way a file, a section, or a line fails to interpret is a
//! typed [`ConditionKind`] recorded beside the tables, so a build is never refused over one
//! malformed line in one mod file and a line is never dropped without saying so.
//!
//! # Module shape
//!
//! - `language` — [`LanguageTag`], the one vocabulary for a Stellaris language. The game
//!   spells one in three places this application reads, and a `.yml` section header is the
//!   third; ingestion parses headers through it rather than declaring a second reading of the
//!   same handful of bytes (D-135's merge rule, performed here as the second of the two
//!   tickets to land).
//! - `detect` — the configured game language, read out of `settings.txt`.
//! - `effective` — explicit override → detected language → English.
//! - `syntax` — the lexical layer, and the three lexical rules that are decisions rather than
//!   restatements: the discarded version suffix, the first-quote-to-last-quote value, and the
//!   refusal to unescape anything.
//! - `table` — the input contract and the effective tables, including why the key-level
//!   provenance vocabulary is this module's own rather than the resolver's.
//! - `ingest` — the two-pass fold, and why the removed files run second.
//! - `markup` — display tokens over one value's text. Ingestion does not call it: what a
//!   value *means* is a separate question from which value the game uses, and answering both
//!   in one pass would tokenize the hundreds of thousands of keys no documentation cites.
//!
//! # Controls shown to go red
//!
//! Every gate here has been broken by hand once and observed failing, then restored. Seeding a
//! forbidden name into a shipped file failed
//! [`localization_names_no_analysis_type`](tests::localization_names_no_analysis_type), and
//! seeding one into a *comment* correctly did not. Seeding one into `markup/scan.rs` failed it
//! too, which is what says the walk reaches a submodule directory rather than stopping at the
//! module root. Lower-casing a section header before
//! [`LanguageTag::parse`] — the leniency that would make `l_English:` a language — failed the
//! two unreadable-header tests and the fault-ordering test. Inverting the fold to first-wins
//! and removing the byte-order-mark strip are recorded with their observed failures in the
//! oracle suite's own ledger, since that is where the records they break are consumed.
//!
//! # What does not enter
//!
//! Resolver internals. `analysis` decides which localization files exist, in what order, and
//! which lost to a path collision or a directory replacement; it hands the result over by
//! building a [`LocalizationInput`], which names only crate-public primitives. Nothing here
//! reaches back for a resolution, and nothing here names the file-selection or provenance
//! types on the other side of that seam — `localization_names_no_analysis_type` scans the
//! shipped source for those names, because the visibility that blocks them today would stop
//! blocking them the day somebody widens it. The Clausewitz parser does not enter either:
//! `.yml` localization and Clausewitz script are separate languages, and feeding one to the
//! other's reader would manufacture failures that describe neither.
//!
//! # What does not leave
//!
//! A [`RawValue`] is source text, not display text. Markup, Static Localization References,
//! Runtime Localization Tokens and `\"` escapes all survive ingestion verbatim; `markup` is
//! the one authority on what any of them mean, and unescaping here would hand it a second
//! dialect to accept. Reference resolution, cycle detection, the fallback chain and the
//! plain-text projection are all later Phase 5 work reading these tables; none of them is
//! anticipated here with a field nothing fills.

pub mod detect;
pub mod effective;
mod ingest;
pub mod language;
// The vocabulary and its seam are complete before the phase tasks that read them — key
// resolution, plain-text projection, and the fallback chain all consume these tokens. Narrow
// the allow as each lands. Precedent: `analysis`'s `parser` and `resolver`.
#[allow(dead_code, unused_imports)]
mod markup;
mod syntax;
mod table;

pub use detect::{DetectedGameLanguage, detect_language, language_from_settings};
pub use effective::{EffectiveLanguage, LanguageSource, derive_effective_language};
pub use ingest::ingest;
pub use language::{LanguageTag, LanguageTagError, language_override_from_document};
pub use syntax::ConditionKind;
pub use table::{
    Condition, EffectiveTables, FileIndex, FileLoss, FileOrigin, IngestedFile, KeyEntry,
    KeyOccurrence, LanguageTable, LocalizationInput, LocalizationKey, RawValue, RemovedFile,
    StreamedFile,
};

/// The version of everything this module decides about what a localization source *means*.
///
/// Homed here, beside the rules it versions, and read by the analysis version vector rather
/// than repeated as a literal there — two literals could drift apart in the commit that
/// changed the semantics, and one constant cannot.
///
/// Bump it for any change to locale identity, the line grammar, the value rule, the merge
/// order, or what a condition means. A previously built revision's documentation is a function
/// of this number, so a silent change would leave a stale revision claiming to be current.
///
/// - 1: the module existed as a declaration only and interpreted nothing.
/// - 2 (Phase 5A, STE-37): localization-file ingestion, locale identity, and the per-language
///   effective tables with per-key provenance and raw-key casualties.
pub const LOCALIZATION_INTERPRETATION_VERSION: u32 = 2;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Names this module must not reach for.
    ///
    /// The first group is the file-selection and provenance vocabulary that stops at the
    /// `analysis` boundary; the second is the Clausewitz parser, which owns a different
    /// language entirely.
    const FORBIDDEN: [&str; 11] = [
        "crate::analysis",
        "analysis::",
        "LocalizationFileStream",
        "ShadowedLocalizationFile",
        "FactProvenance",
        "FactSite",
        "FactKind",
        "Removal",
        "Refusal",
        "jomini",
        "ParsedFile",
    ];

    /// Every line of `source` that names a forbidden type outside a comment.
    fn boundary_violations(source: &str) -> Vec<String> {
        source
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim_start().starts_with("//"))
            .flat_map(|(number, line)| {
                FORBIDDEN
                    .iter()
                    .filter(move |name| line.contains(**name))
                    .map(move |name| format!("line {}: {name}", number + 1))
            })
            .collect()
    }

    /// One `.rs` file of this module, truncated at its test module.
    struct Shipped {
        path: PathBuf,
        source: String,
        /// Whether the file has a test module at all. A file that has one must have been
        /// shortened by the split; a file that has none is scanned whole, which is correct.
        marked: bool,
        /// Length before truncation, so "the split did something" is checkable.
        whole: usize,
    }

    /// Every `.rs` file of this module, at any depth, scoped to its shipped portion.
    ///
    /// Recursive, because a submodule that is a directory is exactly where a boundary would be
    /// crossed unnoticed: a flat walk keeps passing while it silently stops covering the
    /// module. `markup/` is the first such directory and will not be the last.
    ///
    /// Scoped like the resolver's own output gate, for the same reason. Test modules
    /// legitimately name what they exercise, and a gate that tripped over its own detector
    /// would have to be weakened until it detected nothing.
    fn shipped_sources() -> Vec<Shipped> {
        fn walk(directory: &Path, found: &mut Vec<Shipped>) {
            for entry in fs::read_dir(directory).expect("the module directory is readable") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    walk(&path, found);
                    continue;
                }
                if path.extension().is_none_or(|extension| extension != "rs") {
                    continue;
                }
                let whole = fs::read_to_string(&path).expect("a readable source file");
                let source = whole
                    .split_once("#[cfg(test)]")
                    .map_or(whole.as_str(), |(shipped, _)| shipped)
                    .to_owned();
                found.push(Shipped {
                    path,
                    source,
                    marked: whole.contains("#[cfg(test)]"),
                    whole: whole.len(),
                });
            }
        }
        let mut sources = Vec::new();
        walk(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("localization"),
            &mut sources,
        );
        sources
    }

    /// The ingestion boundary, enforced rather than described.
    ///
    /// `analysis` builds this module's input; this module never reaches into `analysis`. The
    /// compiler already blocks it — the handoff types on the other side are internal to
    /// `analysis` — but that is contingent on nobody widening a visibility, and widening one
    /// is exactly the moment the decision is being reversed. A text scan notices the *name*
    /// instead, so the reversal has to be deliberate. Its converse, that `analysis` builds
    /// these tables in exactly one place, is enforced on the other side by the ingestion
    /// stage's own gate.
    #[test]
    fn localization_names_no_analysis_type() {
        let sources = shipped_sources();
        assert!(
            sources.len() >= 4,
            "the module walk found {} files, so the scan may have covered nothing",
            sources.len()
        );
        assert!(
            sources
                .iter()
                .any(|file| file.path.parent().is_some_and(|parent| parent
                    .file_name()
                    .is_some_and(|name| name != "localization"))),
            "the walk reached no submodule directory, so a nested module could be uncovered \
             while this gate stayed green"
        );
        for file in sources {
            assert!(
                !file.marked || file.source.len() < file.whole,
                "{}: the test module marker moved, so the scan covered the whole file",
                file.path.display()
            );
            let found = boundary_violations(&file.source);
            assert!(
                found.is_empty(),
                "{}: {}",
                file.path.display(),
                found.join(", ")
            );
        }
    }

    #[test]
    fn the_scan_detects_a_seeded_analysis_reference() {
        // The negative control, run through the same function the gate uses rather than
        // asserted about a string, and seeded in memory rather than by editing a file, so it
        // runs in ordinary CI and cannot corrupt the source it is about.
        let seeded = concat!(
            "pub struct Input {\n",
            "    // a name in a comment does not count\n",
            "    files: LocalizationFileStream,\n",
            "}\n"
        );
        assert_eq!(
            boundary_violations(seeded),
            ["line 3: LocalizationFileStream"]
        );
    }
}
