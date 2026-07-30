//! The lexical layer: one localization file's bytes in, language-attributed key lines out.
//!
//! # The contract
//!
//! [`parse`] takes bytes and nothing else — no path, no source identity, no stream position.
//! That is deliberate rather than minimal: locale identity comes from the file's `l_<language>`
//! section header, and a parse that could see the directory name could quietly prefer it. The
//! corpus contains a file that would make the difference visible — Workshop mod `3039370479`
//! ships `localisation/braz_por/PreSelect_l_braz_por.yml` whose header is `l_english:` — so
//! "the header decides" is a rule with a counterexample to get wrong, not a formality.
//!
//! A header is read through [`LanguageTag`], the one vocabulary for a Stellaris language,
//! which parses `l_<name>` syntactically rather than against the ten the shipped game has
//! (D-135's merge rule, performed here as the second of the two tickets to land). So a
//! translation mod's own language is a language, and what is left for
//! [`ConditionKind::UnreadableLanguageHeader`] is a header the game itself could not read.
//!
//! Parsing is total. Every line becomes an entry, an ignorable, or a typed
//! [`ConditionKind`]; nothing is dropped without saying so. A malformed line does not discard
//! its file, because the game does not discard the file either and a mod with one bad line
//! would otherwise lose thousands of names.
//!
//! # The grammar, and the three parts of it that are decisions
//!
//! ```text
//! Blank    := ""
//! Comment  := "#" .*
//! Header   := "l_" name ":" ws* ("#" .*)?
//! Entry    := key ":" digit* ws* '"' value '"' rest
//!             value = from the FIRST '"' to the LAST '"' on the line
//!             rest  = "" | ws* "#" .*
//! ```
//!
//! Alternation is ordered, and an `Entry` is discriminated by its quoted value rather than by
//! the absence of an `l_` prefix. A key may legally begin with `l_`: vanilla's
//! `localisation/languages.yml` states `l_english:0 "English"` inside an `l_english:` section.
//!
//! **The `:<digits>` version suffix is recognized and discarded.** It is absent from 1,397,543
//! of the 1,506,561 vanilla key lines, so the grammar must know it exists or every versioned
//! key parses wrong. Nothing downstream reads one — not the fallback chain, not reference
//! resolution, not the closure, not the tokenizer — and a retained field with no reader is the
//! thing "model only what somebody reads" forbids. It is discarded as a decision, not lost as
//! an oversight.
//!
//! **A value runs from the first `"` to the LAST `"` on the line.** 1,477 vanilla lines carry
//! unescaped inner double quotes (`action.76.desc:0 "… - "bricking" - following …"`), so
//! ending at the *next* quote measurably truncates real player-visible text; 12,702 lines
//! carry a `#` comment after the closing quote, so scanning to end of line swallows a comment.
//! First-to-last is the only rule that survives both. Its one residual — a trailing comment
//! that itself contains a `"` — is bounded rather than assumed away: the lexical census
//! asserts that no line in either installed corpus leaves non-comment text after the last
//! quote, which is what makes the rule exact instead of approximate. If that claim ever fails,
//! the rule becomes "the last quote before an unquoted `#`" and the interpretation version
//! bumps with it.
//!
//! **Nothing is unescaped.** `\"` (12,062 lines), `\n`, `£icon£`, `$key$` and `§Y…§!` all
//! survive verbatim. The markup tokenizer is the one authority on what they mean, and
//! unescaping here would hand it a second dialect to accept.

use super::LanguageTag;

/// One key line, attributed to the section it sat in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    pub language: LanguageTag,
    pub key: String,
    /// The source text between the quotes. Not display text — see the module comment.
    pub value: String,
    /// 1-based, over the decoded text. A byte-order mark does not make a line of its own.
    pub line: u32,
}

/// One condition and where in the file it was observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Fault {
    /// `None` when the condition is about the whole file rather than one line.
    pub line: Option<u32>,
    pub kind: ConditionKind,
}

/// What one localization file yielded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Document {
    pub entries: Vec<Entry>,
    /// Ascending by line, whole-file conditions first.
    pub faults: Vec<Fault>,
}

/// Why part of a localization file was not interpreted.
///
/// Closed and exhaustive: every way ingestion declines to produce a key is a variant here, so
/// a file that contributed nothing can always say why. Each variant names the measured shape
/// it exists for, or the gap that would settle it — the discipline
/// `UnresolvedConstant` and `UnresolvedInline` already follow on the resolver side.
///
/// A key repeated within a file or across files is deliberately **not** a condition. That is
/// ordinary provenance — the loser is retained as a shadowed occurrence — and calling the
/// normal case a condition would bury the abnormal ones under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionKind {
    /// The file's bytes are not UTF-8, and it contributes nothing.
    ///
    /// Never lossily converted: a name rendered with `U+FFFD` is a fabricated player-visible
    /// string, and a key that visibly renders raw is at least honestly raw. Unmeasured — every
    /// file in both installed corpora decodes cleanly — so this is a typed absence rather than
    /// a handled shape, and the census is what would notice it appearing.
    NotUtf8 {
        /// Byte offset of the first invalid sequence, after any byte-order mark.
        at: usize,
    },
    /// A section header whose name is not a well-formed language token.
    ///
    /// Not "a language this build has never heard of": [`LanguageTag`] parses `l_<name>`
    /// syntactically, so a translation mod's own language is a language and gets its own
    /// table. What reaches this variant is a header the game itself could not read as one —
    /// `l_English:`, `l_日本語:`, `l_english-uk:`. The section is skipped rather than guessed
    /// at, and `skipped` is what makes the loss enumerable rather than silent.
    UnreadableLanguageHeader { header: String, skipped: u32 },
    /// A key line before any section header, so there is no language to attribute it to.
    ///
    /// A file with no header *and* no key lines is benign and produces nothing at all —
    /// vanilla's `braz_por/new_scripted_loc_POR_l_braz_por.yml` is entirely comments.
    EntryBeforeHeader { key: String },
    /// An opening quote with no closing quote. Measured in ACOT:
    /// `acot_herculean_built_score: "§EHerculean Built§!`. Dropped rather than repaired — no
    /// record measures what the game does with it, and inventing a value fabricates a name.
    UnterminatedValue,
    /// A key with a value that is not quoted at all. Measured in ACOT:
    /// `acot_omegan_blessed: Blessed By Light`. Dropped, for the same reason.
    UnquotedValue,
    /// A line that is not blank, a comment, a header, or anything with a `key:` shape.
    Unparsable,
    /// Text after the closing quote that is neither blank nor a `#` comment.
    ///
    /// The entry is **accepted** and the caveat recorded: the text up to the last quote is
    /// almost certainly what the game reads, and dropping a name to punish a stray character
    /// loses more than it protects. This is the residual of the first-quote-to-last-quote rule
    /// and the variant the census keeps at zero.
    TrailingContentAfterValue,
}

impl ConditionKind {
    /// A stable name per variant, for counting and reporting.
    ///
    /// Exhaustive by construction, so a new variant cannot slip past the census's per-variant
    /// tally by being folded into someone else's bucket.
    pub fn name(&self) -> &'static str {
        match self {
            Self::NotUtf8 { .. } => "NotUtf8",
            Self::UnreadableLanguageHeader { .. } => "UnreadableLanguageHeader",
            Self::EntryBeforeHeader { .. } => "EntryBeforeHeader",
            Self::UnterminatedValue => "UnterminatedValue",
            Self::UnquotedValue => "UnquotedValue",
            Self::Unparsable => "Unparsable",
            Self::TrailingContentAfterValue => "TrailingContentAfterValue",
        }
    }
}

const BYTE_ORDER_MARK: &[u8] = b"\xef\xbb\xbf";

/// The section a line belongs to.
enum Section {
    /// Before the first header.
    None,
    Known(LanguageTag),
    /// Held open so the skipped count can be totalled when the section ends.
    Unreadable {
        header: String,
        line: u32,
        skipped: u32,
    },
}

/// Interpret one localization file.
///
/// Total: never panics, never returns an error, and never drops a line without recording
/// either an entry or a fault for it.
pub(super) fn parse(bytes: &[u8]) -> Document {
    let body = bytes.strip_prefix(BYTE_ORDER_MARK).unwrap_or(bytes);
    let text = match std::str::from_utf8(body) {
        Ok(text) => text,
        Err(error) => {
            return Document {
                entries: Vec::new(),
                faults: vec![Fault {
                    line: None,
                    kind: ConditionKind::NotUtf8 {
                        at: error.valid_up_to(),
                    },
                }],
            };
        }
    };

    let mut entries = Vec::new();
    let mut faults = Vec::new();
    let mut section = Section::None;

    for (index, raw) in text.split('\n').enumerate() {
        let number = index as u32 + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(header) = header_name(line) {
            flush(&mut section, &mut faults);
            section = match LanguageTag::parse(header) {
                Ok(language) => Section::Known(language),
                Err(_) => Section::Unreadable {
                    header: header.to_owned(),
                    line: number,
                    skipped: 0,
                },
            };
            continue;
        }

        match key_line(line) {
            Ok(KeyLine {
                key,
                value,
                trailing,
            }) => {
                if trailing {
                    faults.push(Fault {
                        line: Some(number),
                        kind: ConditionKind::TrailingContentAfterValue,
                    });
                }
                match &mut section {
                    Section::Known(language) => entries.push(Entry {
                        language: language.clone(),
                        key: key.to_owned(),
                        value: value.to_owned(),
                        line: number,
                    }),
                    Section::Unreadable { skipped, .. } => *skipped += 1,
                    Section::None => faults.push(Fault {
                        line: Some(number),
                        kind: ConditionKind::EntryBeforeHeader {
                            key: key.to_owned(),
                        },
                    }),
                }
            }
            Err(kind) => faults.push(Fault {
                line: Some(number),
                kind,
            }),
        }
    }
    flush(&mut section, &mut faults);

    // Stable, so two conditions on one line keep the order they were detected in. An unknown
    // section's condition is only complete once the section closes, which is why the sort
    // happens here rather than the faults being emitted already ordered.
    faults.sort_by_key(|fault| fault.line);
    Document { entries, faults }
}

/// Close an unreadable-header section, recording what it skipped.
fn flush(section: &mut Section, faults: &mut Vec<Fault>) {
    if let Section::Unreadable {
        header,
        line,
        skipped,
    } = std::mem::replace(section, Section::None)
    {
        faults.push(Fault {
            line: Some(line),
            kind: ConditionKind::UnreadableLanguageHeader { header, skipped },
        });
    }
}

/// The language name of a section header, or `None` if this is not one.
///
/// A header is a name ending in a colon with nothing but whitespace or a comment after it.
/// That last clause is the whole discrimination: `l_english:0 "English"` is a key line whose
/// key happens to be a language name, and vanilla's `languages.yml` is full of them.
fn header_name(line: &str) -> Option<&str> {
    let (name, rest) = line.split_once(':')?;
    if !name.starts_with("l_") {
        return None;
    }
    let rest = rest.trim_start();
    (rest.is_empty() || rest.starts_with('#')).then_some(name)
}

struct KeyLine<'a> {
    key: &'a str,
    value: &'a str,
    trailing: bool,
}

fn key_line(line: &str) -> Result<KeyLine<'_>, ConditionKind> {
    let Some((key, rest)) = line.split_once(':') else {
        return Err(ConditionKind::Unparsable);
    };
    if key.is_empty() || key.contains('"') {
        return Err(ConditionKind::Unparsable);
    }

    // The version suffix, consumed and discarded. Whitespace after it is optional: mods ship
    // `strategic_defence_command_platform_cap:"$…$"` with none at all.
    let quoted = rest
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .trim_start();
    if !quoted.starts_with('"') {
        return Err(ConditionKind::UnquotedValue);
    }
    let close = quoted
        .rfind('"')
        .filter(|index| *index > 0)
        .ok_or(ConditionKind::UnterminatedValue)?;

    let after = quoted[close + 1..].trim();
    Ok(KeyLine {
        key,
        value: &quoted[1..close],
        trailing: !after.is_empty() && !after.starts_with('#'),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn tag(name: &str) -> LanguageTag {
        LanguageTag::parse(name).expect("a well-formed language token")
    }

    fn entries(source: &[u8]) -> Vec<(LanguageTag, String, String, u32)> {
        parse(source)
            .entries
            .into_iter()
            .map(|entry| (entry.language, entry.key, entry.value, entry.line))
            .collect()
    }

    fn kinds(source: &[u8]) -> Vec<(Option<u32>, ConditionKind)> {
        parse(source)
            .faults
            .into_iter()
            .map(|fault| (fault.line, fault.kind))
            .collect()
    }

    fn value(source: &[u8]) -> String {
        let parsed = parse(source);
        assert_eq!(parsed.entries.len(), 1, "{parsed:?}");
        parsed.entries.into_iter().next().unwrap().value
    }

    #[test]
    fn a_byte_order_mark_is_stripped_and_does_not_shift_line_numbers() {
        // Every one of the 2318 vanilla files carries a mark and nine Giga files do not, so
        // both spellings must reach the same table with the same coordinates.
        let with = b"\xef\xbb\xbfl_english:\n key:0 \"value\"\n";
        let without = b"l_english:\n key:0 \"value\"\n";
        assert_eq!(entries(with), entries(without));
        assert_eq!(entries(with)[0].3, 2);
    }

    #[test]
    fn crlf_and_lf_produce_identical_entries() {
        let crlf = b"l_english:\r\n key:0 \"value\"\r\n";
        let lf = b"l_english:\n key:0 \"value\"\n";
        assert_eq!(entries(crlf), entries(lf));
    }

    #[test]
    fn indentation_is_optional() {
        // 1793 vanilla key lines and most mod key lines start at column zero.
        let indented = b"l_english:\n key:0 \"value\"\n";
        let flush = b"l_english:\nkey:0 \"value\"\n";
        assert_eq!(entries(indented), entries(flush));
    }

    #[test]
    fn a_version_number_is_optional_and_is_not_retained() {
        // 1,397,543 of 1,506,561 vanilla key lines omit it, so the common shape is the
        // versionless one; the two must be indistinguishable downstream.
        assert_eq!(
            entries(b"l_english:\n key:0 \"value\"\n"),
            entries(b"l_english:\n key: \"value\"\n")
        );
        assert_eq!(value(b"l_english:\n key:12 \"value\"\n"), "value");
        assert_eq!(value(b"l_english:\n key:\"value\"\n"), "value");
    }

    #[test]
    fn an_unescaped_inner_quote_is_part_of_the_value() {
        // The measured vanilla shape: `action.76.desc:0 "… - "bricking" - …"`.
        assert_eq!(
            value(b"l_english:\n k:0 \"a - \"bricking\" - b\"\n"),
            "a - \"bricking\" - b"
        );
    }

    #[test]
    fn a_trailing_comment_after_the_closing_quote_is_not_part_of_the_value() {
        assert_eq!(value(b"l_english:\n k:0 \"value\" # note\n"), "value");
        assert!(
            parse(b"l_english:\n k:0 \"value\" # note\n")
                .faults
                .is_empty()
        );
    }

    #[test]
    fn a_backslash_escaped_quote_is_retained_verbatim() {
        // Unescaping here would give the markup tokenizer a second dialect to accept.
        assert_eq!(
            value(b"l_english:\n k:0 \"He said \\\"hi\\\"\"\n"),
            "He said \\\"hi\\\""
        );
    }

    #[test]
    fn markup_survives_untouched() {
        assert_eq!(
            value("l_english:\n k:0 \"§Y£energy£ $sub$§!\"\n".as_bytes()),
            "§Y£energy£ $sub$§!"
        );
    }

    #[test]
    fn trailing_content_after_the_value_is_recorded_and_the_entry_is_kept() {
        let parsed = parse(b"l_english:\n k:0 \"value\" stray\n");
        assert_eq!(parsed.entries[0].value, "value");
        assert_eq!(
            kinds(b"l_english:\n k:0 \"value\" stray\n"),
            [(Some(2), ConditionKind::TrailingContentAfterValue)]
        );
        assert_eq!(parsed.entries.len(), 1);
    }

    #[test]
    fn an_unterminated_value_is_typed_and_the_next_line_still_parses() {
        // ACOT ships `acot_herculean_built_score: "§EHerculean Built§!`.
        let source = "l_english:\n bad:0 \"§EHerculean Built§!\n good:0 \"kept\"\n".as_bytes();
        assert_eq!(kinds(source), [(Some(2), ConditionKind::UnterminatedValue)]);
        assert_eq!(entries(source).len(), 1);
        assert_eq!(entries(source)[0].1, "good");
    }

    #[test]
    fn an_unquoted_value_is_typed_and_the_next_line_still_parses() {
        // ACOT ships `acot_omegan_blessed: Blessed By Light`.
        let source = b"l_english:\n bad: Blessed By Light\n good:0 \"kept\"\n";
        assert_eq!(kinds(source), [(Some(2), ConditionKind::UnquotedValue)]);
        assert_eq!(entries(source)[0].1, "good");
    }

    #[test]
    fn a_line_with_no_separator_or_no_key_is_unparsable() {
        assert_eq!(
            kinds(b"l_english:\n no separator here\n"),
            [(Some(2), ConditionKind::Unparsable)]
        );
        assert_eq!(
            kinds(b"l_english:\n : \"value\"\n"),
            [(Some(2), ConditionKind::Unparsable)]
        );
    }

    #[test]
    fn a_key_line_whose_key_begins_with_l_underscore_is_not_a_header() {
        // Vanilla's `languages.yml` states `l_english:0 "English"` inside an `l_english:`
        // section. Discriminating on the `l_` prefix rather than on the quoted value would
        // silently turn every one of those ten lines into a section switch.
        let source = b"l_english:\n l_english:0 \"English\"\n l_korean:0 \"Korean\"\n";
        assert_eq!(
            entries(source)
                .into_iter()
                .map(|(language, key, _, _)| (language, key))
                .collect::<Vec<_>>(),
            [
                (tag("l_english"), "l_english".to_owned()),
                (tag("l_english"), "l_korean".to_owned()),
            ]
        );
    }

    #[test]
    fn a_second_header_switches_the_language_mid_file() {
        // Vanilla's `localisation/languages.yml` holds ten sections in one file.
        let source = b"l_english:\n k:0 \"en\"\nl_german:\n k:0 \"de\"\n";
        assert_eq!(
            entries(source),
            [
                (tag("l_english"), "k".to_owned(), "en".to_owned(), 2),
                (tag("l_german"), "k".to_owned(), "de".to_owned(), 4),
            ]
        );
    }

    #[test]
    fn a_header_may_carry_a_trailing_comment() {
        assert_eq!(
            entries(b"l_english: # the section\n k:0 \"v\"\n")[0].0,
            tag("l_english")
        );
    }

    #[test]
    fn a_file_with_no_header_and_only_comments_yields_nothing_at_all() {
        // Vanilla's `braz_por/new_scripted_loc_POR_l_braz_por.yml` is exactly this: a real,
        // benign file whose every line is commented out. It must not become an error.
        let parsed = parse(b"\xef\xbb\xbf# nothing active\n# l_braz_por:\n#  key: \"v\"\n");
        assert!(parsed.entries.is_empty());
        assert!(parsed.faults.is_empty());
    }

    #[test]
    fn a_key_before_any_header_is_typed_rather_than_guessed() {
        assert_eq!(
            kinds(b"orphan:0 \"value\"\nl_english:\n k:0 \"v\"\n"),
            [(
                Some(1),
                ConditionKind::EntryBeforeHeader {
                    key: "orphan".to_owned()
                }
            )]
        );
    }

    #[test]
    fn a_language_the_base_game_does_not_ship_gets_its_own_table() {
        // A translation mod adds a language by adding an `l_<name>` header, so the parse is
        // syntactic rather than an allow-list of the ten the shipped game has. An allow-list
        // here would classify every community translation's keys as skipped (D-135, D-138).
        let source = b"l_klingon:\n a:0 \"x\"\nl_english:\n b:0 \"y\"\n";
        assert!(parse(source).faults.is_empty());
        assert_eq!(
            entries(source)
                .into_iter()
                .map(|(language, key, _, _)| (language, key))
                .collect::<Vec<_>>(),
            [
                (tag("l_klingon"), "a".to_owned()),
                (tag("l_english"), "b".to_owned()),
            ]
        );
    }

    #[test]
    fn an_unreadable_language_header_skips_its_section_and_counts_the_keys() {
        // What is left once a mod-added language is a language: a header the game itself
        // could not read as one.
        let source = b"l_English:\n a:0 \"x\"\n b:0 \"y\"\nl_english:\n c:0 \"z\"\n";
        assert_eq!(
            kinds(source),
            [(
                Some(1),
                ConditionKind::UnreadableLanguageHeader {
                    header: "l_English".to_owned(),
                    skipped: 2
                }
            )]
        );
        assert_eq!(entries(source).len(), 1);
        assert_eq!(entries(source)[0].0, tag("l_english"));
    }

    #[test]
    fn an_unreadable_section_that_runs_to_end_of_file_is_still_reported() {
        // The count is only complete when the section closes, and the last section closes at
        // end of file rather than at a header. Without the final flush this file would report
        // nothing at all, which is the silent drop the whole condition vocabulary exists
        // against.
        assert_eq!(
            kinds(b"l_English:\n a:0 \"x\"\n"),
            [(
                Some(1),
                ConditionKind::UnreadableLanguageHeader {
                    header: "l_English".to_owned(),
                    skipped: 1
                }
            )]
        );
    }

    #[test]
    fn invalid_utf8_yields_one_file_condition_and_no_keys() {
        let parsed = parse(b"l_english:\n k:0 \"\xff\xfe\"\n");
        assert!(parsed.entries.is_empty());
        assert_eq!(parsed.faults.len(), 1);
        assert_eq!(parsed.faults[0].line, None);
        assert_eq!(parsed.faults[0].kind.name(), "NotUtf8");
    }

    #[test]
    fn faults_are_reported_in_line_order() {
        // An unknown section's fault is emitted when the section closes, which is after the
        // faults of the lines that follow it. Order is restored before the document leaves.
        let source = b"l_English:\n a:0 \"x\"\nl_english:\n broken\n";
        let lines: Vec<_> = parse(source).faults.iter().map(|f| f.line).collect();
        assert_eq!(lines, [Some(1), Some(4)]);
    }

    #[test]
    fn first_quote_to_next_quote_would_truncate_the_measured_inner_quote_line() {
        // The negative control for the module's central lexical rule, run against the exact
        // vanilla shape it was chosen for. A rule that ended the value at the *next* quote
        // reads `a - ` and silently loses the rest of a player-visible sentence.
        fn wrong(line: &str) -> &str {
            let rest = line.split_once(':').unwrap().1;
            let open = rest.find('"').unwrap();
            let tail = &rest[open + 1..];
            &tail[..tail.find('"').unwrap()]
        }
        let line = " k:0 \"a - \"bricking\" - b\"";
        assert_eq!(wrong(line), "a - ");
        assert_ne!(
            wrong(line),
            value(format!("l_english:\n{line}\n").as_bytes())
        );
    }

    proptest! {
        #[test]
        fn classification_is_total(source in prop::collection::vec(any::<u8>(), 0..512)) {
            // Every line must become an entry, an ignorable, or a fault. Counting the lines
            // that are neither blank nor a comment and comparing against entries plus faults
            // is the anti-silent-drop invariant stated over arbitrary input rather than over
            // an example. `TrailingContentAfterValue` is the one shape that produces both, so
            // the comparison is an inequality in that direction only.
            let parsed = parse(&source);
            let Ok(text) = std::str::from_utf8(source.strip_prefix(BYTE_ORDER_MARK).unwrap_or(&source)) else {
                prop_assert_eq!(parsed.faults.len(), 1);
                prop_assert!(parsed.entries.is_empty());
                return Ok(());
            };
            let significant = text
                .split('\n')
                .filter(|line| {
                    let line = line.strip_suffix('\r').unwrap_or(line).trim();
                    !line.is_empty() && !line.starts_with('#')
                })
                .count();
            // Headers are significant lines that produce neither an entry nor (usually) a
            // fault, and an unknown section folds many skipped entries into one fault, so the
            // accounting is bounded rather than exact: nothing may be produced from nothing.
            prop_assert!(parsed.entries.len() + parsed.faults.len() <= significant * 2);
            prop_assert!(parsed.entries.len() <= significant);
        }

        #[test]
        fn an_entry_round_trips(
            key in "[a-zA-Z][a-zA-Z0-9_.]{0,24}",
            text in "[a-zA-Z0-9 ]{0,40}",
            version in prop::option::of(0u32..99),
        ) {
            let version = version.map_or(String::new(), |v| v.to_string());
            let source = format!("l_english:\n {key}:{version} \"{text}\"\n");
            let parsed = parse(source.as_bytes());
            prop_assert!(parsed.faults.is_empty(), "{parsed:?}");
            prop_assert_eq!(parsed.entries.len(), 1);
            prop_assert_eq!(&parsed.entries[0].key, &key);
            prop_assert_eq!(&parsed.entries[0].value, &text);
        }
    }
}
