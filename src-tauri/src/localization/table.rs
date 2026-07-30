//! What ingestion consumes and what it produces: the per-language effective tables, their
//! per-key provenance, and the input contract `analysis` fills in.
//!
//! # Why localization states its own input
//!
//! The resolver decides which localization files exist and in what order the game reads them,
//! and hands that over as bytes it has deliberately not interpreted. Its handoff types are
//! internal to `analysis`, so the question of who owns the shape they cross in has to be
//! answered rather than inherited. It is answered here: **the consumer declares the question**
//! ([`LocalizationInput`]), and `analysis` builds one from its own answer in a single adapter.
//! Only crate-public primitives cross, so nothing in this module can reach a resolver internal
//! even by accident, and a text-scan gate in the parent module notices the *name* on the day
//! somebody widens a visibility.
//!
//! # Why the provenance vocabulary is localization's own
//!
//! The resolver's `FactSite` cannot express key-level provenance and should not be stretched
//! to. It has no line, and a line is the only thing that orders two statements of one key
//! inside one file; its stream ordinal already means "definition ordinal within its file" and
//! is hashed under that meaning; a removed file has no stream position at all, so occurrences
//! in shadowed files could not be ordered against surviving ones; and it has no notion of a
//! language section, while `localisation/languages.yml` puts ten in one file. Three of the
//! resolver's five fact kinds — inherited, defaulted, duplicate — have no meaning for a
//! localization key at all.
//!
//! So the authority splits cleanly rather than duplicating. **File selection is the resolver's
//! fact**: which file lost and why, carried across once in [`FileLoss`] and never re-derived.
//! **Key attribution is localization's fact**: which line of which file supplied which value,
//! and which keys died with a removed file. Nothing else in the crate can compute the second,
//! because nothing else reads `.yml`.

use std::borrow::Borrow;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use crate::canonical::path::LogicalPath;
use crate::source::snapshot::{SourceBytes, SourceKind};

use super::LanguageTag;
use super::syntax::ConditionKind;

// --- Input ---

/// One localization file that survived selection, in the order the game reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamedFile {
    pub source: SourceKind,
    pub logical: LogicalPath,
    pub bytes: SourceBytes,
}

/// One localization file that never reached the stream, with the bytes it would have supplied.
///
/// Both halves are needed: `loss` explains why the file lost, and `bytes` are what make the
/// keys that disappeared with it knowable at all. Keeping only the path would leave the
/// casualties unknowable after file selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedFile {
    pub source: SourceKind,
    pub logical: LogicalPath,
    pub bytes: SourceBytes,
    pub loss: FileLoss,
}

/// Everything ingestion reads.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LocalizationInput {
    /// Surviving files in game load order.
    ///
    /// **Position in this vector is the stream order.** A structural invariant, where a
    /// per-file ordinal would be a promise two entries could break by carrying the same
    /// number — and one nothing downstream could check.
    pub streamed: Vec<StreamedFile>,
    /// Files removed before any stream existed. Order is irrelevant; ingestion sorts them, so
    /// determinism is owned here rather than assumed of the caller.
    pub removed: Vec<RemovedFile>,
}

// --- The file dimension ---

/// Identity of one input file within one ingest.
///
/// **Not a precedence.** Streamed files take `0..n` in stream order and removed files take
/// `n..n+m`; the fold never considers a removed file for a winner, so the index order cannot
/// be mistaken for load order. Provenance carries an index rather than a path and a source
/// because it attaches to roughly a million and a half key lines on the real corpus, and
/// because "which keys did this file lose" is then a grouping rather than a scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileIndex(u32);

/// One input file, as provenance refers to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestedFile {
    pub source: SourceKind,
    pub logical: LogicalPath,
    pub origin: FileOrigin,
}

/// How a file reached — or failed to reach — the ingestion stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOrigin {
    /// Read at this position in the localization stream.
    Streamed { order: u32 },
    /// Removed before the stream, so it can supply shadowed values and casualties but never a
    /// winner. Distinct from a low stream position: a file that never entered did not lose to
    /// one, and giving it a position would be plausible-looking provenance, which is worse
    /// than a coarse one.
    Removed { loss: FileLoss },
}

/// Why a localization file never reached the stream.
///
/// Localization's restatement of the resolver's removal vocabulary, produced by the one
/// exhaustive mapping in the `analysis` adapter — so a removal mechanism added there fails to
/// compile before it can silently arrive here as something else. The two mechanisms stay apart
/// for the reason the resolver keeps them apart: a collision names the source that won, a
/// directory replacement names the declaration that excluded the file, and a reader that could
/// not tell them apart would not know whether the mod shipped a replacement at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileLoss {
    ReplacedDirectory { declaration: LogicalPath },
    ShadowedByPathCollision { winner: SourceKind },
}

// --- The key dimension ---

/// A localization key, exactly as the source line spelled it. Case-sensitive, never folded.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalizationKey(String);

impl LocalizationKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for LocalizationKey {
    fn from(key: String) -> Self {
        Self(key)
    }
}

impl Borrow<str> for LocalizationKey {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// The source text between a value's quotes.
///
/// **Not display text.** Markup (`§Y…§!`, `£icon£`), Static Localization References (`$key$`),
/// Runtime Localization Tokens and `\"` escapes all survive verbatim; the markup tokenizer is
/// the one authority on what any of them mean. A newtype rather than a `String` so a consumer
/// that renders this straight to a reader has to say so.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RawValue(String);

impl RawValue {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for RawValue {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// One statement of one key, in one language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyOccurrence {
    pub value: RawValue,
    pub file: FileIndex,
    /// 1-based line within the decoded file. The coordinate the resolver's provenance cannot
    /// express: it is the only thing that orders two statements inside one file, and the only
    /// thing an Analysis Issue can point a reader at.
    pub line: u32,
}

/// Everything known about one key in one language.
///
/// There is no `Option` here. A key with no winner is not an entry — it is a casualty, or it
/// is nothing at all, and those are different answers a consumer must be able to tell apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEntry {
    winner: KeyOccurrence,
    shadowed: Vec<KeyOccurrence>,
}

impl KeyEntry {
    /// The value the game uses: the last streamed statement, in stream then line order.
    pub fn winner(&self) -> &KeyOccurrence {
        &self.winner
    }

    /// Everything that lost, ascending by file index then line: earlier streamed statements
    /// first, then statements from removed files.
    ///
    /// Both losses are one list, with the distinction carried on the file's
    /// [`FileOrigin`] where a consumer reads it — that is what separates "a later file
    /// deliberately overrode this" from "this went down with a whole shadowed file".
    pub fn shadowed(&self) -> &[KeyOccurrence] {
        &self.shadowed
    }

    /// Whether more than one *file* stated this key. The floor under "a mod renamed this".
    ///
    /// A file that states a key twice is not a contest — it is one file's own redundancy, and
    /// the base game has 144 such statements across its own localization. Reporting those as
    /// contests would bury the case an Analysis Issue is for under the case it is not: a
    /// second file deliberately overriding a name. So the comparison is against the winner's
    /// file, not against the mere existence of a loser.
    ///
    /// A floor rather than the whole answer, deliberately. Cross-*source* is a narrower
    /// question again, and one a caller can ask from [`shadowed`](Self::shadowed) plus
    /// [`EffectiveTables::file`]; naming that judgement here would decide for a consumer that
    /// does not exist yet.
    pub fn is_contested(&self) -> bool {
        self.shadowed
            .iter()
            .any(|occurrence| occurrence.file != self.winner.file)
    }
}

/// One language's effective table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LanguageTable {
    entries: BTreeMap<LocalizationKey, KeyEntry>,
    casualties: BTreeMap<LocalizationKey, Vec<KeyOccurrence>>,
}

impl LanguageTable {
    /// Record a statement from a surviving file. Last wins.
    ///
    /// The rule lives here rather than in the fold because it is what makes the type's
    /// invariant true: the incumbent is demoted in place, so `shadowed` stays ascending by
    /// construction and no caller has to remember to keep it that way. Callers must supply
    /// files in stream order and lines in file order; that is the fold's one obligation.
    pub(super) fn record_streamed(&mut self, key: LocalizationKey, occurrence: KeyOccurrence) {
        match self.entries.entry(key) {
            Entry::Vacant(vacant) => {
                vacant.insert(KeyEntry {
                    winner: occurrence,
                    shadowed: Vec::new(),
                });
            }
            Entry::Occupied(mut occupied) => {
                let entry = occupied.get_mut();
                let displaced = std::mem::replace(&mut entry.winner, occurrence);
                entry.shadowed.push(displaced);
            }
        }
    }

    /// Record a statement from a removed file: a shadowed value if the key survives elsewhere,
    /// a raw-key casualty if it does not.
    ///
    /// Only correct after every surviving file has been recorded, which is why ingestion runs
    /// the removed files in a second pass. Casualty status is decidable only against the
    /// complete effective table — a key is a casualty relative to what survived, not relative
    /// to the file it was lost from.
    pub(super) fn record_removed(&mut self, key: LocalizationKey, occurrence: KeyOccurrence) {
        match self.entries.get_mut(key.as_str()) {
            Some(entry) => entry.shadowed.push(occurrence),
            None => self.casualties.entry(key).or_default().push(occurrence),
        }
    }

    /// The effective entry for one key, or `None` when nothing surviving states it.
    ///
    /// `None` plus a [`casualty`](Self::casualty) means "the game renders the raw key, and here
    /// is the file that took it away". `None` with no casualty means nobody ever stated it.
    /// Keeping those apart is what lets the fallback chain distinguish a shadowing accident
    /// from an undefined key.
    pub fn get(&self, key: &str) -> Option<&KeyEntry> {
        self.entries.get(key)
    }

    /// Statements of a key that exists **only** in removed files, and therefore renders in
    /// game as its own identifier.
    pub fn casualty(&self, key: &str) -> Option<&[KeyOccurrence]> {
        self.casualties.get(key).map(Vec::as_slice)
    }

    /// Every effective key, in canonical order. What the cited-key closure seeds from.
    pub fn keys(&self) -> impl Iterator<Item = &LocalizationKey> {
        self.entries.keys()
    }

    pub fn entries(&self) -> impl Iterator<Item = (&LocalizationKey, &KeyEntry)> {
        self.entries.iter()
    }

    pub fn casualties(&self) -> impl Iterator<Item = (&LocalizationKey, &[KeyOccurrence])> {
        self.casualties
            .iter()
            .map(|(key, lost)| (key, lost.as_slice()))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One condition, and where it was observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub file: FileIndex,
    /// `None` when the condition is about the whole file rather than one line.
    pub line: Option<u32>,
    pub kind: ConditionKind,
}

/// Every language's effective table, plus the provenance and conditions behind them.
///
/// Ordering is total and host-independent throughout: sorted maps rather than hash maps, and
/// occurrence lists ascending by `(file, line)`. Nothing computes an identity over this yet —
/// the persisted shape is the pruned multilingual structure a revision preserves, and choosing
/// its encoding before the cited-key closure exists would pin a format to a working set — but
/// determinism is established now so that identity is possible without revisiting the fold.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EffectiveTables {
    files: Vec<IngestedFile>,
    languages: BTreeMap<LanguageTag, LanguageTable>,
    conditions: Vec<Condition>,
}

impl EffectiveTables {
    pub(super) fn new(files: Vec<IngestedFile>) -> Self {
        Self {
            files,
            languages: BTreeMap::new(),
            conditions: Vec::new(),
        }
    }

    pub(super) fn table_mut(&mut self, language: LanguageTag) -> &mut LanguageTable {
        self.languages.entry(language).or_default()
    }

    pub(super) fn record_condition(&mut self, condition: Condition) {
        self.conditions.push(condition);
    }

    /// Languages with at least one effective key.
    ///
    /// A language present only as casualties is deliberately excluded: offering it would offer
    /// a language whose every key renders as its own identifier, which is the failure the
    /// casualty list exists to make visible rather than to hide behind a menu entry.
    pub fn available(&self) -> impl Iterator<Item = &LanguageTag> {
        self.languages
            .iter()
            .filter(|(_, table)| !table.is_empty())
            .map(|(language, _)| language)
    }

    pub fn table(&self, language: &LanguageTag) -> Option<&LanguageTable> {
        self.languages.get(language)
    }

    pub fn tables(&self) -> impl Iterator<Item = (&LanguageTag, &LanguageTable)> {
        self.languages.iter()
    }

    /// One key in one language.
    ///
    /// Returns the entry rather than the text because a consumer that resolves a `$key$`
    /// reference needs the provenance of the value it resolved to, not only the value.
    pub fn get(&self, language: &LanguageTag, key: &str) -> Option<&KeyEntry> {
        self.table(language)?.get(key)
    }

    /// One key in every language that has it, in canonical language order.
    ///
    /// The unit the cited-key transitive closure walks: the closure is taken across every
    /// language at once, because a reference present in one translation may be absent from
    /// another, and a per-language closure would lose text on the very language switch that
    /// preserving every language exists to protect.
    pub fn across_languages<'a>(
        &'a self,
        key: &'a str,
    ) -> impl Iterator<Item = (&'a LanguageTag, &'a KeyEntry)> {
        self.languages
            .iter()
            .filter_map(move |(language, table)| Some((language, table.get(key)?)))
    }

    /// The file a provenance handle refers to. The only way a [`FileIndex`] becomes a path.
    pub fn file(&self, index: FileIndex) -> &IngestedFile {
        &self.files[index.0 as usize]
    }

    pub fn files(&self) -> impl Iterator<Item = (FileIndex, &IngestedFile)> {
        self.files
            .iter()
            .enumerate()
            .map(|(index, file)| (FileIndex(index as u32), file))
    }

    /// Raw-key casualties grouped by the file they were lost with.
    ///
    /// A derived view computed on demand, not a second stored index: it runs once per build
    /// over a handful of removed files, and a stored copy would be a second authority on a
    /// fact the per-language tables already own. This grouping is what makes the loss
    /// *scoped* — the same shape the oracle measured, where a mod's same-named file blanked
    /// its vanilla counterpart's other keys and left every other vanilla file untouched — and
    /// it is the count that separates a deliberate rename from hundreds of casualties.
    /// Each key appears once per file that lost it, however many times that file stated it.
    /// A set rather than a list because the consumer counts casualties, and a key a file
    /// happened to state twice is one name the player will not see, not two.
    pub fn casualties_by_file(
        &self,
    ) -> BTreeMap<FileIndex, BTreeMap<&LanguageTag, BTreeSet<&str>>> {
        let mut grouped: BTreeMap<FileIndex, BTreeMap<&LanguageTag, BTreeSet<&str>>> =
            BTreeMap::new();
        for (language, table) in self.tables() {
            for (key, lost) in table.casualties() {
                for occurrence in lost {
                    grouped
                        .entry(occurrence.file)
                        .or_default()
                        .entry(language)
                        .or_default()
                        .insert(key.as_str());
                }
            }
        }
        grouped
    }

    /// Every way a file, a section, or a line was not fully interpreted.
    ///
    /// Ascending by `(file, line)`. Nothing in this module renders them: their consumers are
    /// the Analysis Issue that will carry them to a reader, and the lexical census that
    /// measures them across the installed corpora.
    pub fn conditions(&self) -> &[Condition] {
        &self.conditions
    }
}

/// The index a file will have once `streamed` then `removed` are laid out end to end.
pub(super) fn file_index(position: usize) -> FileIndex {
    FileIndex(position as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(name: &str) -> LanguageTag {
        LanguageTag::parse(name).expect("a well-formed language token")
    }

    fn occurrence(text: &str, file: usize, line: u32) -> KeyOccurrence {
        KeyOccurrence {
            value: text.to_owned().into(),
            file: file_index(file),
            line,
        }
    }

    fn removed(logical: &str) -> IngestedFile {
        IngestedFile {
            source: SourceKind::VanillaContent,
            logical: LogicalPath::parse(logical).expect("a fixture path"),
            origin: FileOrigin::Removed {
                loss: FileLoss::ShadowedByPathCollision {
                    winner: SourceKind::TargetMod,
                },
            },
        }
    }

    fn streamed(logical: &str, order: u32) -> IngestedFile {
        IngestedFile {
            source: SourceKind::TargetMod,
            logical: LogicalPath::parse(logical).expect("a fixture path"),
            origin: FileOrigin::Streamed { order },
        }
    }

    #[test]
    fn recording_a_second_statement_demotes_the_incumbent_in_order() {
        let mut table = LanguageTable::default();
        table.record_streamed("k".to_owned().into(), occurrence("first", 0, 2));
        table.record_streamed("k".to_owned().into(), occurrence("second", 1, 7));
        table.record_streamed("k".to_owned().into(), occurrence("third", 2, 1));

        let entry = table.get("k").expect("an entry");
        assert_eq!(entry.winner().value.as_str(), "third");
        assert_eq!(
            entry
                .shadowed()
                .iter()
                .map(|o| o.value.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"],
            "the demotion order is what keeps `shadowed` ascending without the caller sorting"
        );
        assert!(entry.is_contested());
    }

    #[test]
    fn a_key_one_file_states_twice_is_not_contested() {
        // The distinction the accessor's name claims. Vanilla holds 144 shadowed statements of
        // its own, and reporting a file's internal redundancy as a rename would bury the case
        // an Analysis Issue exists for under the case it is not.
        let mut table = LanguageTable::default();
        table.record_streamed("k".to_owned().into(), occurrence("first", 0, 2));
        table.record_streamed("k".to_owned().into(), occurrence("second", 0, 9));

        let entry = table.get("k").expect("an entry");
        assert_eq!(entry.shadowed().len(), 1, "the loser is still retained");
        assert!(!entry.is_contested());

        // The same key restated from a second file is a contest, so the guard is a comparison
        // and not a blanket false.
        table.record_streamed("k".to_owned().into(), occurrence("third", 1, 1));
        assert!(table.get("k").unwrap().is_contested());
    }

    #[test]
    fn a_removed_statement_shadows_a_surviving_key_and_orphans_an_absent_one() {
        let mut table = LanguageTable::default();
        table.record_streamed("kept".to_owned().into(), occurrence("mod", 0, 2));
        table.record_removed("kept".to_owned().into(), occurrence("vanilla", 1, 2));
        table.record_removed("lost".to_owned().into(), occurrence("vanilla", 1, 3));

        assert_eq!(table.get("kept").unwrap().shadowed().len(), 1);
        assert!(table.casualty("kept").is_none());
        assert!(
            table.get("lost").is_none(),
            "a casualty has no winner: absent-with-a-casualty and absent-entirely are \
             different answers, and the fallback chain has to tell them apart"
        );
        assert_eq!(table.casualty("lost").unwrap()[0].value.as_str(), "vanilla");
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn available_excludes_a_language_that_is_only_casualties() {
        // Offering it would offer a language whose every key renders as its own identifier,
        // which is the failure the casualty list exists to expose rather than to hide behind a
        // menu entry.
        let mut tables = EffectiveTables::new(vec![
            streamed("localisation/english/a_l_english.yml", 0),
            removed("localisation/german/b_l_german.yml"),
        ]);
        tables
            .table_mut(tag("l_english"))
            .record_streamed("k".to_owned().into(), occurrence("v", 0, 2));
        tables
            .table_mut(tag("l_german"))
            .record_removed("k".to_owned().into(), occurrence("v", 1, 2));

        assert_eq!(tables.available().collect::<Vec<_>>(), [&tag("l_english")]);
        assert!(
            tables.table(&tag("l_german")).is_some(),
            "the table is still reachable — the casualties are the point of keeping it"
        );
    }

    #[test]
    fn across_languages_walks_one_key_in_canonical_language_order() {
        let mut tables = EffectiveTables::new(vec![streamed("localisation/languages.yml", 0)]);
        for language in [tag("l_korean"), tag("l_english"), tag("l_german")] {
            tables
                .table_mut(language)
                .record_streamed("k".to_owned().into(), occurrence("v", 0, 2));
        }
        assert_eq!(
            tables
                .across_languages("k")
                .map(|(language, _)| language)
                .collect::<Vec<_>>(),
            [&tag("l_english"), &tag("l_german"), &tag("l_korean")],
            "insertion order must not reach a consumer; the closure walks a total order, \
             which for an open set of languages is the tag's own"
        );
        assert_eq!(tables.across_languages("absent").count(), 0);
    }

    #[test]
    fn casualties_group_by_the_file_that_took_them() {
        let mut tables = EffectiveTables::new(vec![
            removed("localisation/english/one_l_english.yml"),
            removed("localisation/english/two_l_english.yml"),
        ]);
        let english = tables.table_mut(tag("l_english"));
        english.record_removed("a".to_owned().into(), occurrence("A", 0, 2));
        english.record_removed("b".to_owned().into(), occurrence("B", 0, 3));
        english.record_removed("c".to_owned().into(), occurrence("C", 1, 2));

        let grouped = tables.casualties_by_file();
        assert_eq!(grouped.len(), 2);
        assert_eq!(
            grouped[&file_index(0)][&tag("l_english")],
            BTreeSet::from(["a", "b"])
        );
        assert_eq!(
            grouped[&file_index(1)][&tag("l_english")],
            BTreeSet::from(["c"])
        );
    }

    #[test]
    fn a_file_index_resolves_to_the_file_it_names() {
        let tables = EffectiveTables::new(vec![
            streamed("localisation/english/a_l_english.yml", 0),
            removed("localisation/english/b_l_english.yml"),
        ]);
        assert_eq!(
            tables.file(file_index(1)).logical.as_str(),
            "localisation/english/b_l_english.yml"
        );
        assert_eq!(
            tables.files().map(|(index, _)| index).collect::<Vec<_>>(),
            [file_index(0), file_index(1)],
            "an index is a position in this list, and nothing else"
        );
    }
}
