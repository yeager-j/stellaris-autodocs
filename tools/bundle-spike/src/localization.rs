//! Localization ingestion and the ordered stream that decides which value wins.
//!
//! This is a different language from Clausewitz script with a different collision rule, and
//! `docs/technical-design.md:349` gives it its own module owner for that reason. The rule the
//! resolver spike established is not the one script uses:
//!
//! > Script registries and sprites resolve in one global logical-path order with no layer.
//! > Localization resolves in layer order — every mod file after every Vanilla file — and
//! > then every `replace/` file after that, from any position
//! > (`docs/spikes/resolver-evaluation.md:498`).
//!
//! Within that stream, the last loaded key wins. A mod file named `00_…` therefore loses to
//! vanilla in a script registry and beats it here, which is exactly the kind of asymmetry a
//! generic "mods override the base game" implementation gets wrong in one direction and never
//! notices.
//!
//! Selection matters as much as ordering. An exact path collision shadows the *whole* vanilla
//! file, so every key the winning file omits renders as its raw key rather than falling back
//! to the shadowed value. That is handled in `resolve`, which owns file selection; this
//! module receives the surviving files already chosen.

use crate::corpus::SourceFile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Which phase of the ordered stream a file belongs to.
///
/// Ordinals are explicit because they are the ordering, not a consequence of declaration
/// order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Phase {
    Vanilla = 0,
    Mod = 1,
    /// `localisation/replace/…` from any source. Genuine priority rather than a position:
    /// the resolver spike's `r15` observed a `replace/` file win against a mod that loaded
    /// first.
    Replace = 2,
}

pub fn phase_of(kind: crate::corpus::ContributorKind, logical: &str) -> Phase {
    if logical.contains("/replace/") {
        Phase::Replace
    } else if kind == crate::corpus::ContributorKind::Vanilla {
        Phase::Vanilla
    } else {
        Phase::Mod
    }
}

/// One localization file, positioned in the stream.
pub struct StreamFile<'a> {
    pub file: &'a SourceFile,
    pub phase: Phase,
    pub contributor: &'a str,
}

/// One language's effective key table plus what it took to build it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Language {
    /// Effective key to value, after the whole stream has been applied.
    pub entries: BTreeMap<String, String>,
    /// How many times a later file replaced an earlier file's value.
    ///
    /// A count rather than the shadowed values themselves. Retaining every shadowed value
    /// across eleven languages would multiply the largest single input in the corpus, and
    /// `docs/technical-design.md:446` asks a revision to persist "the data required for
    /// fallback behavior" — the effective table and the language set — not the resolution
    /// history. Resolution-time provenance stays in this record; it is not materialized into
    /// the bundle. That is a deliberate narrowing, and it is stated here so a later reader
    /// does not mistake it for an oversight.
    pub shadowed: usize,
}

/// Every available language, keyed by the language tag the file itself declares.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Localization {
    pub languages: BTreeMap<String, Language>,
    /// Files whose declared language did not match their directory.
    pub directory_mismatches: Vec<String>,
    /// Files with no `l_<language>:` header at all, which contribute nothing.
    pub headerless: Vec<String>,
}

impl Localization {
    /// Selected language, then English, then the raw key.
    ///
    /// The three steps are independent, which matters: a key absent from *every* language
    /// renders raw, because Stellaris has no fallback language for a key nothing defines
    /// (`docs/spikes/resolver-evaluation.md:106`). Returning the key itself rather than an
    /// empty string is what makes that visible instead of silently blank.
    pub fn resolve<'a>(&'a self, language: &str, key: &'a str) -> Resolved<'a> {
        if let Some(value) = self.languages.get(language).and_then(|l| l.entries.get(key)) {
            return Resolved {
                text: value,
                fallback: Fallback::Selected,
            };
        }
        if language != "english" {
            if let Some(value) = self.languages.get("english").and_then(|l| l.entries.get(key)) {
                return Resolved {
                    text: value,
                    fallback: Fallback::English,
                };
            }
        }
        Resolved {
            text: key,
            fallback: Fallback::RawKey,
        }
    }

    pub fn total_entries(&self) -> usize {
        self.languages.values().map(|l| l.entries.len()).sum()
    }

    pub fn total_value_bytes(&self) -> u64 {
        self.languages
            .values()
            .flat_map(|language| language.entries.iter())
            .map(|(key, value)| (key.len() + value.len()) as u64)
            .sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    Selected,
    English,
    RawKey,
}

pub struct Resolved<'a> {
    pub text: &'a str,
    pub fallback: Fallback,
}

/// Apply the ordered stream and return the effective tables.
///
/// Files are sorted by phase, then by normalized logical-path bytes within the phase, so
/// filesystem traversal order can never contribute to which value wins.
pub fn ingest(mut stream: Vec<StreamFile<'_>>) -> std::io::Result<Localization> {
    stream.sort_by(|a, b| {
        a.phase
            .cmp(&b.phase)
            .then_with(|| a.contributor.cmp(b.contributor))
            .then_with(|| a.file.logical.as_bytes().cmp(b.file.logical.as_bytes()))
    });

    let mut localization = Localization::default();
    for entry in &stream {
        let bytes = std::fs::read(&entry.file.absolute)?;
        let parsed = parse(&bytes);

        let Some(language) = parsed.language else {
            localization.headerless.push(entry.file.logical.clone());
            continue;
        };

        if let Some(directory) = directory_language(&entry.file.logical) {
            if directory != language {
                localization
                    .directory_mismatches
                    .push(format!("{} declares {language}", entry.file.logical));
            }
        }

        let table = localization.languages.entry(language).or_default();
        for (key, value) in parsed.entries {
            if table.entries.insert(key, value).is_some() {
                table.shadowed += 1;
            }
        }
    }

    localization.directory_mismatches.sort();
    localization.headerless.sort();
    Ok(localization)
}

/// `localisation/english/foo_l_english.yml` and `localisation/replace/english/…` both yield
/// `english`.
fn directory_language(logical: &str) -> Option<&str> {
    let mut components = logical.split('/');
    components.next()?; // `localisation`
    match components.next()? {
        "replace" => components.next(),
        directory => Some(directory),
    }
}

#[derive(Debug, Default)]
pub struct ParsedFile {
    /// From the `l_<language>:` header, not from the directory.
    ///
    /// The header is what the game reads. A file under `english/` that declares `l_french:`
    /// contributes French, and treating the directory as authoritative would silently move
    /// its keys into the wrong table.
    pub language: Option<String>,
    pub entries: Vec<(String, String)>,
}

/// Parse one Stellaris localization file.
///
/// Deliberately byte-oriented and permissive about everything except the two things that
/// decide content: the language header and the `key:version "value"` shape. Shipped files
/// contain a UTF-8 byte order mark, inconsistent indentation, comments in three positions,
/// and occasional lines that are none of the above; rejecting a whole file over one of those
/// would discard thousands of good keys to punish a formatting choice the game accepts.
pub fn parse(bytes: &[u8]) -> ParsedFile {
    let text = String::from_utf8_lossy(strip_bom(bytes));
    let mut parsed = ParsedFile::default();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if parsed.language.is_none() {
            if let Some(language) = line.strip_prefix("l_").and_then(|rest| rest.strip_suffix(':')) {
                parsed.language = Some(language.trim().to_owned());
                continue;
            }
        }

        let Some((key, value)) = split_entry(line) else {
            continue;
        };
        parsed.entries.push((key, value));
    }

    parsed
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes)
}

/// ` some_key:0 "the value"` → `("some_key", "the value")`.
///
/// The value is taken from the first quote to the last quote on the line, rather than to the
/// first closing quote. Localization values contain quotes — nested `"…"` inside a sentence,
/// and Stellaris's own `£icon£` and `$VARIABLE$` markup beside them — and stopping at the
/// first one truncates the value silently, which is worse than failing to read it at all.
fn split_entry(line: &str) -> Option<(String, String)> {
    let colon = line.find(':')?;
    let key = line[..colon].trim();
    if key.is_empty() || !key.bytes().all(is_key_byte) {
        return None;
    }

    let rest = &line[colon + 1..];
    let open = rest.find('"')?;
    // The version number between the colon and the quote is not retained: it is a translator
    // workflow marker, and nothing downstream reads it.
    let close = rest.rfind('"')?;
    if close <= open {
        return None;
    }

    Some((key.to_owned(), rest[open + 1..close].to_owned()))
}

fn is_key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'\'' | b'@')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_header_the_file_declares_not_the_one_expected() {
        let parsed = parse(b"\xef\xbb\xbfl_french:\n tech_a:0 \"Alpha\"\n");
        assert_eq!(parsed.language.as_deref(), Some("french"));
        assert_eq!(parsed.entries, vec![("tech_a".into(), "Alpha".into())]);
    }

    #[test]
    fn a_value_containing_quotes_is_not_truncated_at_the_first_one() {
        let parsed = parse(b"l_english:\n k:0 \"the \"Enigmalith\" itself\"\n");
        assert_eq!(parsed.entries[0].1, "the \"Enigmalith\" itself");
    }

    #[test]
    fn comments_and_blank_lines_contribute_nothing() {
        let parsed = parse(b"l_english:\n# comment\n\n k:0 \"v\"\n");
        assert_eq!(parsed.entries.len(), 1);
    }

    #[test]
    fn fallback_is_selected_then_english_then_the_raw_key() {
        let mut localization = Localization::default();
        localization.languages.insert(
            "english".into(),
            Language {
                entries: BTreeMap::from([
                    ("shared".to_owned(), "English shared".to_owned()),
                    ("only_english".to_owned(), "English only".to_owned()),
                ]),
                shadowed: 0,
            },
        );
        localization.languages.insert(
            "french".into(),
            Language {
                entries: BTreeMap::from([("shared".to_owned(), "Partagé".to_owned())]),
                shadowed: 0,
            },
        );

        assert_eq!(localization.resolve("french", "shared").text, "Partagé");
        assert_eq!(
            localization.resolve("french", "only_english").text,
            "English only"
        );

        let missing = localization.resolve("french", "defined_nowhere");
        assert_eq!(missing.text, "defined_nowhere");
        assert_eq!(missing.fallback, Fallback::RawKey);
    }

    #[test]
    fn replace_outranks_a_mod_file_which_outranks_vanilla() {
        use crate::corpus::ContributorKind::{TargetMod, Vanilla};

        assert_eq!(phase_of(Vanilla, "localisation/english/a.yml"), Phase::Vanilla);
        assert_eq!(phase_of(TargetMod, "localisation/english/a.yml"), Phase::Mod);
        assert_eq!(
            phase_of(TargetMod, "localisation/replace/english/a.yml"),
            Phase::Replace
        );
        // The asymmetry worth stating: a `replace/` file from Vanilla would also be last.
        assert_eq!(
            phase_of(Vanilla, "localisation/replace/english/a.yml"),
            Phase::Replace
        );
        assert!(Phase::Vanilla < Phase::Mod && Phase::Mod < Phase::Replace);
    }
}
