//! The lexical census: what the installed corpora's `.yml` localization actually looks like,
//! and whether the rules `localization::syntax` chose still fit it.
//!
//! # Why this exists and what it is not
//!
//! No oracle record measures `.yml` lexis. The records establish which files load in which
//! order and what a path collision destroys; none of them says whether a value ends at the
//! first closing quote or the last, whether the `:<version>` suffix is mandatory, or whether a
//! file must carry a byte-order mark. Those questions were settled by measuring the corpus,
//! and a measurement that lives only in a commit message is a claim nothing can check again.
//! This is where it is checkable.
//!
//! It reads the corpus through the production path — `localization_stage::tables` over a real
//! resolution — and through localization's public surface, never through a regex of its own. A
//! second reader of the same bytes would be a second authority on the grammar, which is the
//! argument [`resolver::census`](crate::analysis::resolver) already makes about tokenization.
//! The two byte-level counters it does keep (a leading mark, CRLF terminators, and lines whose
//! key separator is followed by a digit) count bytes rather than interpreting them, and exist
//! only to prove the shapes the rules were chosen for are still present.
//!
//! # Running it
//!
//! The corpora are a licensed local installation, so this is not part of the ordinary suite:
//!
//! ```text
//! cargo test --features test-support localization_lexical_census -- --ignored --nocapture
//! ```
//!
//! Roots come from [`corpora`], the same overrides every other corpus run uses. The
//! measurement and its reading are recorded in `docs/spikes/localization-lexical-census.md`
//! (STE-37).
//!
//! # What the claims assert, and why none can pass vacuously
//!
//! Counts are printed rather than pinned — they are a property of whichever build and mod
//! version is installed, and the spike note is where a specific reading belongs. Four claims
//! are asserted, because each would be a *changed conclusion* rather than a changed number:
//!
//! 1. **Every file contributed a key or a condition.** No localization file is read and
//!    silently dropped. This is the anti-silent-drop invariant at corpus scale, where the
//!    property tests state it over generated input.
//! 2. **The base game produces no condition at all.** A fault in Paradox's own files means the
//!    grammar is wrong, not that the file is. It is also what makes claim 3's zero meaningful:
//!    the same detector reports faults over the mod corpus.
//! 3. **The first-quote-to-last-quote rule leaves no residual.** Nothing sits after a value's
//!    last quote but a comment, and no captured value carries the signature of having
//!    swallowed one. Together those bound the rule's only known ambiguity. If either ever
//!    fails, the rule becomes "the last quote before an unquoted `#`" and
//!    `LOCALIZATION_INTERPRETATION_VERSION` bumps with it.
//! 4. **Every shape the rules exist for still occurs, and the detectors are live.** A
//!    byte-order mark, CRLF terminators, key lines with and without a version suffix, values
//!    with an unescaped inner quote, and at least one malformed line in the mod corpus.
//!    Without this, claims 2 and 3 could be reporting a dead detector rather than a clean
//!    corpus.
//! 5. **The base game still ships exactly the ten languages its `languages.yml` declares.**
//!    Not a constraint — [`LanguageTag`] parses `l_<name>` syntactically, so an eleventh
//!    language is a table rather than an error, and a translation mod's own language is a
//!    language. The claim is a *reading* of the pinned build, and it is what a header-parsing
//!    regression would break: if headers stopped being read, this set would shrink long before
//!    any key count looked wrong. A game update that adds a language fails it, which is the
//!    signal to re-read this note rather than a defect.
//!
//! One shape is deliberately *not* claimed here: a file with no byte-order mark. Every file in
//! both pinned corpora carries one, and the mark-less form was measured elsewhere — nine
//! Gigastructural Engineering files ship without it (STE-37). Asserting its presence here
//! would be asserting something about a mod the pinned corpora do not contain, so the rule is
//! exercised instead by `fixtures/resolver/localization-vanilla/…/main_2_l_english.yml`, which
//! is committed mark-less and CRLF and runs in ordinary CI. The count is still reported, so a
//! corpus that gained one would be visible.

use std::collections::{BTreeMap, BTreeSet};

use crate::analysis::corpora::{self, establish_corpus};
use crate::analysis::resolver::resolve;
use crate::localization::{EffectiveTables, FileIndex, KeyOccurrence, LanguageTag};
use crate::source::SourceKind;

/// What the raw bytes of the localization stream look like, before interpretation.
///
/// Byte counting, not parsing: a leading mark is three bytes at offset zero, a CRLF file
/// contains `\r\n`, and a versioned key line has an ASCII digit immediately after its first
/// colon. Each exists to prove a rule's subject is still in the corpus.
#[derive(Debug, Default)]
struct Encodings {
    files: usize,
    byte_order_mark: usize,
    no_byte_order_mark: usize,
    crlf_files: usize,
    versioned_lines: usize,
    unversioned_lines: usize,
    /// Files holding nothing a key could be read from — a header and comments, and no more.
    /// Real and common in the base game: 31 such files ship in the 4.4.6 corpus.
    empty_files: usize,
}

impl Encodings {
    /// Count the shapes, and return how many lines in this file could hold a key.
    ///
    /// "Could hold a key" is deliberately coarser than the grammar: any non-comment line with
    /// both a colon and a quote. It is a floor for what interpretation owes an answer about,
    /// and being coarse is what makes it independent — a counter that agreed with the parser
    /// by construction could not catch the parser losing a line.
    fn observe(&mut self, bytes: &[u8]) -> usize {
        self.files += 1;
        if bytes.starts_with(b"\xef\xbb\xbf") {
            self.byte_order_mark += 1;
        } else {
            self.no_byte_order_mark += 1;
        }
        if bytes.windows(2).any(|pair| pair == b"\r\n") {
            self.crlf_files += 1;
        }
        let mut candidates = 0;
        for line in bytes.split(|byte| *byte == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let trimmed = line.trim_ascii_start();
            if trimmed.is_empty() || trimmed.starts_with(b"#") || !trimmed.contains(&b'"') {
                continue;
            }
            match trimmed.iter().position(|byte| *byte == b':') {
                Some(colon) if trimmed.get(colon + 1).is_some_and(u8::is_ascii_digit) => {
                    self.versioned_lines += 1;
                    candidates += 1;
                }
                Some(_) => {
                    self.unversioned_lines += 1;
                    candidates += 1;
                }
                None => {}
            }
        }
        if candidates == 0 {
            self.empty_files += 1;
        }
        candidates
    }
}

/// What interpretation produced, per contributor.
#[derive(Debug, Default)]
struct Interpretation {
    keys: BTreeMap<LanguageTag, usize>,
    casualties: usize,
    shadowed: usize,
    conditions: BTreeMap<&'static str, usize>,
    /// Values holding a `"` that no backslash escapes — the measured shape that rules out
    /// ending a value at its *next* quote.
    inner_quote_values: usize,
    /// Values holding a quote followed by a comment marker: the signature of a trailing
    /// comment that itself contained a quote, which the last-quote rule would have swallowed.
    /// The rule's one residual, and the thing claim 3 bounds.
    swallowed_comment_values: usize,
}

impl Interpretation {
    fn observe_value(&mut self, occurrence: &KeyOccurrence) {
        let text = occurrence.value.as_str();
        if text.replace("\\\"", "").contains('"') {
            self.inner_quote_values += 1;
        }
        if swallowed_comment(text) {
            self.swallowed_comment_values += 1;
        }
    }

    fn total_conditions(&self) -> usize {
        self.conditions.values().sum()
    }
}

/// Whether a captured value looks like it ran past a closing quote into a comment.
fn swallowed_comment(text: &str) -> bool {
    text.match_indices('"').any(|(index, _)| {
        !text[..index].ends_with('\\') && text[index + 1..].trim_start().starts_with('#')
    })
}

/// Which files supplied at least one key, and which supplied at least one condition.
fn contributing_files(tables: &EffectiveTables) -> (BTreeSet<FileIndex>, BTreeSet<FileIndex>) {
    let mut keyed = BTreeSet::new();
    for (_, table) in tables.tables() {
        for (_, entry) in table.entries() {
            keyed.insert(entry.winner().file);
            keyed.extend(entry.shadowed().iter().map(|occurrence| occurrence.file));
        }
        for (_, lost) in table.casualties() {
            keyed.extend(lost.iter().map(|occurrence| occurrence.file));
        }
    }
    let faulted = tables
        .conditions()
        .iter()
        .map(|condition| condition.file)
        .collect();
    (keyed, faulted)
}

/// The census STE-37 asked for. See the module comment for how to run it and what it claims.
#[test]
#[ignore = "requires an installed Stellaris and ACOT; run with --ignored"]
fn localization_lexical_census() {
    let vanilla_corpus = corpora::vanilla();
    let acot_corpus = corpora::acot();
    let (vanilla_source, mut warnings) = establish_corpus(&vanilla_corpus);
    let (acot_source, acot_warnings) = establish_corpus(&acot_corpus);
    warnings.extend(acot_warnings);

    let resolution = resolve(vanilla_source.snapshot(), acot_source.snapshot());
    let stream = resolution
        .localization_files()
        .expect("the localization file stream resolves over the installed corpora");
    let mut encodings: BTreeMap<SourceKind, Encodings> = BTreeMap::new();
    // Keyed by identity rather than by position: ingestion sorts the removed files, so their
    // indices need not follow the order the resolver's projection produced them in.
    let mut candidates: BTreeMap<(SourceKind, &str), usize> = BTreeMap::new();
    for file in &stream.files {
        let counted = encodings
            .entry(file.source)
            .or_default()
            .observe(file.bytes.as_slice());
        candidates.insert((file.source, file.logical.as_str()), counted);
    }
    for file in &stream.shadowed_files {
        let (Some(source), Some(logical)) = (
            file.provenance.site.source(),
            file.provenance.site.logical(),
        ) else {
            continue;
        };
        let counted = encodings
            .entry(source)
            .or_default()
            .observe(file.bytes.as_slice());
        candidates.insert((source, logical.as_str()), counted);
    }

    let tables = super::tables(&resolution).expect("the localization stage resolves");
    let mut interpretations: BTreeMap<SourceKind, Interpretation> = BTreeMap::new();
    for (language, table) in tables.tables() {
        for (_, entry) in table.entries() {
            let winner = interpretations
                .entry(tables.file(entry.winner().file).source)
                .or_default();
            *winner.keys.entry(language.clone()).or_default() += 1;
            winner.observe_value(entry.winner());
            for occurrence in entry.shadowed() {
                let held = interpretations
                    .entry(tables.file(occurrence.file).source)
                    .or_default();
                held.shadowed += 1;
                held.observe_value(occurrence);
            }
        }
        for (_, lost) in table.casualties() {
            for occurrence in lost {
                let held = interpretations
                    .entry(tables.file(occurrence.file).source)
                    .or_default();
                held.casualties += 1;
                held.observe_value(occurrence);
            }
        }
    }
    for condition in tables.conditions() {
        *interpretations
            .entry(tables.file(condition.file).source)
            .or_default()
            .conditions
            .entry(condition.kind.name())
            .or_default() += 1;
    }

    for warning in &warnings {
        println!("warning: {warning}");
    }
    println!(
        "{} surviving files, {} removed, languages available: {}",
        stream.files.len(),
        stream.shadowed_files.len(),
        tables
            .available()
            .map(LanguageTag::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    );
    for (source, measured) in &encodings {
        println!("{source:?} encodings: {measured:?}");
    }
    for (source, measured) in &interpretations {
        println!("{source:?} interpretation:");
        for (language, keys) in &measured.keys {
            println!("  {language}: {keys} effective keys");
        }
        println!(
            "  {} shadowed values, {} raw-key casualties",
            measured.shadowed, measured.casualties
        );
        println!(
            "  {} values with an unescaped inner quote, {} with a swallowed-comment signature",
            measured.inner_quote_values, measured.swallowed_comment_values
        );
        for (kind, count) in &measured.conditions {
            println!("  condition {kind}: {count}");
        }
    }
    // Every condition, named. There are few enough to read: a corpus that produced hundreds
    // would be telling you the grammar is wrong, and the first twenty would say so too.
    for condition in tables.conditions().iter().take(20) {
        println!(
            "condition {} at {}:{}",
            condition.kind.name(),
            tables.file(condition.file).logical.as_str(),
            condition
                .line
                .map_or("?".to_owned(), |line| line.to_string())
        );
    }
    for (index, keys) in tables.casualties_by_file() {
        println!(
            "casualties in {}: {}",
            tables.file(index).logical.as_str(),
            keys.values().map(BTreeSet::len).sum::<usize>()
        );
    }

    let vanilla = interpretations
        .get(&SourceKind::VanillaContent)
        .expect("the base game contributes localization");
    let target = interpretations
        .get(&SourceKind::TargetMod)
        .expect("the Target Mod contributes localization");
    let vanilla_bytes = &encodings[&SourceKind::VanillaContent];

    // Claim 4 first, because it is what stops the two zeros below from being a dead detector.
    assert!(
        target.total_conditions() > 0,
        "no malformed line anywhere in the mod corpus, which is a broken detector rather than \
         a clean corpus — the measured shapes are an unterminated value and an unquoted one"
    );
    assert!(
        vanilla_bytes.byte_order_mark > 0,
        "no file carries a byte-order mark, so stripping one is a rule nothing exercises: \
         {vanilla_bytes:?}"
    );
    assert!(
        vanilla_bytes.crlf_files > 0,
        "no file uses CRLF terminators, so stripping one is a rule nothing exercises: \
         {vanilla_bytes:?}"
    );
    assert!(
        vanilla_bytes.versioned_lines > 0 && vanilla_bytes.unversioned_lines > 0,
        "both key-line shapes must occur, or the optional version suffix is a rule nothing \
         exercises: {vanilla_bytes:?}"
    );
    assert!(
        vanilla.inner_quote_values > 0,
        "no value carries an unescaped inner quote, so the corpus no longer contains the shape \
         that rules out ending a value at its next quote"
    );

    // Claim 5: a reading of the pinned build, and the guard a header-parsing regression trips
    // long before a key count looks wrong. Not a constraint on what a language may be — an
    // eleventh is a table, not an error — so a failure here is a signal to re-read the census
    // note, not a defect to route around.
    let shipped: BTreeSet<&str> = vanilla.keys.keys().map(LanguageTag::as_str).collect();
    assert_eq!(
        shipped,
        BTreeSet::from([
            "l_braz_por",
            "l_english",
            "l_french",
            "l_german",
            "l_japanese",
            "l_korean",
            "l_polish",
            "l_russian",
            "l_simp_chinese",
            "l_spanish",
        ]),
        "the base game's language set moved; `localisation/languages.yml` is the authority and \
         docs/spikes/localization-lexical-census.md is what needs re-reading"
    );

    // Claim 1: nothing that could hold a key was read and silently dropped.
    //
    // Not "every file produced something": 31 base-game files are a language header and
    // comments, and producing nothing from them is the right answer. The claim is that a file
    // producing nothing had nothing to produce, measured by an independent byte-level floor.
    let (keyed, faulted) = contributing_files(&tables);
    let silent: Vec<_> = tables
        .files()
        .filter(|(index, _)| !keyed.contains(index) && !faulted.contains(index))
        .filter(|(_, file)| {
            candidates
                .get(&(file.source, file.logical.as_str()))
                .is_some_and(|counted| *counted > 0)
        })
        .map(|(_, file)| file.logical.as_str().to_owned())
        .collect();
    assert!(
        silent.is_empty(),
        "{} localization files hold a key-shaped line and produced neither a key nor a \
         condition: {silent:?}",
        silent.len()
    );

    // Claim 2: the base game's own files parse clean.
    assert_eq!(
        vanilla.total_conditions(),
        0,
        "the base game's own localization produced conditions, which means the grammar is \
         wrong rather than the files: {:?}",
        vanilla.conditions
    );

    // Claim 3: the value rule has no residual.
    for (source, measured) in &interpretations {
        assert_eq!(
            measured.conditions.get("TrailingContentAfterValue"),
            None,
            "{source:?} has text after a value's last quote that is not a comment, so the \
             first-quote-to-last-quote rule no longer covers the corpus"
        );
        assert_eq!(
            measured.swallowed_comment_values, 0,
            "{source:?} has a value that swallowed a trailing comment containing a quote — the \
             rule's one residual is no longer vacuous, and it must become 'the last quote \
             before an unquoted #' with an interpretation-version bump"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_swallowed_comment_signature_matches_only_a_swallowed_comment() {
        // The detector claim 3 rests on. It must fire on the shape the last-quote rule would
        // capture from `k:0 "value" # see "this"`, and stay quiet on the ordinary measured
        // shapes — an unescaped inner quote, an escaped one, and a `#` in running text.
        assert!(swallowed_comment("value\" # see \"this"));
        assert!(swallowed_comment("value\"# terse"));
        assert!(!swallowed_comment("a - \"bricking\" - b"));
        assert!(!swallowed_comment("He said \\\" #1"));
        assert!(!swallowed_comment("issue #1 is open"));
        assert!(!swallowed_comment("plain"));
    }

    #[test]
    fn the_encoding_counters_read_the_measured_shapes() {
        let mut counted = Encodings::default();
        assert_eq!(
            counted.observe(b"\xef\xbb\xbfl_english:\n k:0 \"v\"\n j: \"v\"\n # note\n"),
            2
        );
        assert_eq!(counted.observe(b"l_english:\r\n k:1 \"v\"\r\n"), 1);
        assert_eq!(
            counted.observe(b"\xef\xbb\xbfl_english:\n # nothing to read\n"),
            0,
            "a header-and-comments file holds no key-shaped line, which is what makes \
             producing nothing from it the right answer rather than a silent drop"
        );
        assert_eq!(counted.files, 3);
        assert_eq!(counted.byte_order_mark, 2);
        assert_eq!(counted.no_byte_order_mark, 1);
        assert_eq!(counted.crlf_files, 1);
        assert_eq!(counted.versioned_lines, 2);
        assert_eq!(counted.unversioned_lines, 1);
        assert_eq!(counted.empty_files, 1);
    }
}
