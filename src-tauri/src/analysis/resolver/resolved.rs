//! What the resolver hands the rest of `analysis`: effective definitions addressed by key,
//! and the provenance of every fact in them.
//!
//! **Nothing here names a parse.** A resolved registry answers "which definition does the
//! game use for this key, and where did each fact come from" — a question whose answer must
//! not depend on how many files were read, which of them recovered from a fault, or what a
//! `ParsedFile` is shaped like. `resolved_output_names_no_parse` is the gate; it is a text
//! scan rather than a proof, for the same reason the parser's own gate is
//! (`super::super::parser`).
//!
//! A definition *body* is still the application-owned parsed value model. That is
//! deliberate: it is `analysis`'s own representation of a Clausewitz value, not a
//! parser-library type, and re-modelling it here would be a second authority on what a
//! Clausewitz value is — a wrapper that only delegates, with two definitions of equality to
//! keep in step. What the gate forbids is the *file-level* model escaping: the whole-file
//! type, its faults, and the per-file evidence bookkeeping that belongs to the read rather
//! than to the answer.

use crate::canonical::path::LogicalPath;
use crate::source::SourceKind;
use std::collections::BTreeMap;

use super::super::parser::{Container, Value};

/// The identifier a registry addresses a definition by.
///
/// A newtype rather than `String` because the key is not always the block name — ship
/// components key on an inner `key` field, and sprites on a `name` field inside a
/// `spriteType` (`docs/spikes/resolver-evaluation.md`, registry matrix). A row's
/// [`KeyRule`](super::registry::KeyRule) decides what the key is; downstream code only ever
/// needs it to be a stable identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::analysis) struct DefinitionKey(String);

impl DefinitionKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where a fact sits in a content family's semantic stream.
///
/// The coordinates the canonicalization rules require of provenance — "actual semantic
/// resolution order, source identity, logical path, and definition ordinal"
/// (docs/technical-design.md, "Canonicalization and numeric representation"). `order` is the
/// stream index, **not** a source rank: the whole point of the r10 finding is that Vanilla
/// and the Target Mod interleave in one order, so a position is meaningful and a layer is
/// not.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) struct StreamPosition {
    /// Index into the family's semantic stream. Files, not definitions.
    pub order: u32,
    pub source: SourceKind,
    pub logical: LogicalPath,
    /// The definition's ordinal within its file, as the parse produced it.
    pub ordinal: u32,
}

/// Why a file was removed before any stream existed.
///
/// Recorded rather than collapsed to "gone" because the two mechanisms owe a reader
/// different things. D-098 requires the directory-replacement row's provenance to name "the
/// replacing declaration and every excluded source", so an excluded file has to be able to
/// say *which* declaration excluded it; a path collision has no declaration to name and
/// instead records the source that won.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) enum Removal {
    ReplacedDirectory { declaration: LogicalPath },
    ShadowedByPathCollision { winner: SourceKind },
}

/// Where a fact came from.
///
/// Not every fact has a stream position, and the accessors return `Option` because of it. A
/// file removed by common file selection never entered a stream, so it has no resolution
/// order and no definition ordinal; a value the Resolution Profile supplied has no source
/// file at all. Inventing coordinates for either would be the kind of plausible-looking
/// provenance that is worse than a coarse one — a `Defaulted` fact carrying a stream
/// position tells a reader that a mod's file supplied a value it never mentioned.
///
/// This is also what lets the r6 fact — "this vanilla file contributed nothing, including
/// keys the winner never mentions" — be recorded at all.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) enum FactSite {
    Stream(StreamPosition),
    RemovedBySelection {
        source: SourceKind,
        logical: LogicalPath,
        removal: Removal,
    },
    /// A value the row itself declared. No source stated it, which is the fact.
    DeclaredDefault {
        registry: &'static str,
    },
}

impl FactSite {
    /// The contributing source, or `None` when no source contributed — the difference
    /// between "Vanilla supplied this" and "nothing did, the profile did".
    pub fn source(&self) -> Option<SourceKind> {
        match self {
            Self::Stream(position) => Some(position.source),
            Self::RemovedBySelection { source, .. } => Some(*source),
            Self::DeclaredDefault { .. } => None,
        }
    }

    pub fn logical(&self) -> Option<&LogicalPath> {
        match self {
            Self::Stream(position) => Some(&position.logical),
            Self::RemovedBySelection { logical, .. } => Some(logical),
            Self::DeclaredDefault { .. } => None,
        }
    }
}

/// How a fact came to be in an effective definition.
///
/// The five kinds D-098 requires. They are not a severity scale: `Duplicate` and `Shadowed`
/// record what *lost*, and a consumer that only reads the effective fields still gets a
/// complete definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::analysis) enum FactKind {
    /// The winning definition stated it.
    Contributed,
    /// Carried from a definition this one displaced, because the row's field rule inherits
    /// absent fields. No shipped row selects that rule; see
    /// [`Replacement`](super::registry::Replacement).
    Inherited,
    /// Supplied by the row's declared defaults because no definition stated it.
    Defaulted,
    /// A repeat registration of a key already held. Recorded whether the repeat won or lost,
    /// because "there were two" is the fact — which one survived is the effective field.
    Duplicate,
    /// Lost outright: a definition displaced by the row's repeat rule, or a whole file
    /// removed by common file selection.
    Shadowed,
}

impl FactKind {
    pub const ALL: [Self; 5] = [
        Self::Contributed,
        Self::Inherited,
        Self::Defaulted,
        Self::Duplicate,
        Self::Shadowed,
    ];
}

/// One recorded fact about how a definition, or one field of it, was decided.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) struct FactProvenance {
    pub kind: FactKind,
    /// The field this fact is about, or `None` when it is about the whole definition — a
    /// displaced duplicate, or a file shadowed before any key was read.
    pub field: Option<String>,
    pub site: FactSite,
}

/// One field of the effective definition, and how it got there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct EffectiveField {
    pub field: String,
    pub value: Value,
    /// One of `Contributed`, `Inherited`, or `Defaulted`. The other two kinds describe what
    /// lost and so never name an effective field.
    pub kind: FactKind,
    pub site: FactSite,
}

/// The effective definition for one key.
///
/// `fields` is a derived view of `body` plus whatever the row's field rule and defaults
/// supplied. Two representations, with the synchronization rule stated rather than assumed:
/// both are built once during resolution and neither is mutable afterwards, so they cannot
/// drift. `body` is retained because it is the only thing carrying real source ranges, which
/// Source Excerpts need; `fields` is retained because the effective definition is not always
/// what one block states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct ResolvedDefinition {
    pub key: DefinitionKey,
    /// Where the winning definition was read from.
    pub position: StreamPosition,
    /// The winning definition exactly as parsed.
    pub body: Value,
    /// Effective fields in source order, supplements last.
    pub fields: Vec<EffectiveField>,
    /// What did not survive for this key: displaced duplicates, and the definitions they
    /// shadowed.
    pub displaced: Vec<FactProvenance>,
}

impl ResolvedDefinition {
    /// The first effective value for `field`, or `None` when the definition has none.
    ///
    /// "Has none" is the whole of the omitted-`potential` result: under whole-object
    /// replacement an absent field is absent, never inherited from what it displaced
    /// (`docs/spikes/resolver-evaluation.md`, "Technology redefinition is whole-object
    /// replacement").
    pub fn field(&self, field: &str) -> Option<&Value> {
        self.fields
            .iter()
            .find(|candidate| candidate.field == field)
            .map(|candidate| &candidate.value)
    }

    pub fn states(&self, field: &str) -> bool {
        self.field(field).is_some()
    }

    /// Every fact about this definition, in a total order: effective fields in source order,
    /// then what lost.
    pub fn provenance(&self) -> Vec<FactProvenance> {
        self.fields
            .iter()
            .map(|field| FactProvenance {
                kind: field.kind,
                field: Some(field.field.clone()),
                site: field.site.clone(),
            })
            .chain(self.displaced.iter().cloned())
            .collect()
    }
}

/// One registry's effective content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct ResolvedRegistry {
    pub registry: &'static str,
    pub definitions: BTreeMap<DefinitionKey, ResolvedDefinition>,
    /// Facts belonging to no surviving key: files removed by common file selection, and so
    /// every key they would have contributed.
    pub removed_files: Vec<FactProvenance>,
}

impl ResolvedRegistry {
    pub fn get(&self, key: &str) -> Option<&ResolvedDefinition> {
        self.definitions.get(&DefinitionKey::new(key))
    }

    pub fn keys(&self) -> Vec<&str> {
        self.definitions.keys().map(DefinitionKey::as_str).collect()
    }

    /// Every fact this registry recorded, definitions first in key order, then the files
    /// that never reached a stream.
    pub fn provenance(&self) -> Vec<FactProvenance> {
        self.definitions
            .values()
            .flat_map(ResolvedDefinition::provenance)
            .chain(self.removed_files.iter().cloned())
            .collect()
    }
}

/// The fields a definition body states, or `None` when the body is not an object.
///
/// A scripted constant's body is a bare scalar, so "the fields of this definition" is a
/// question with no answer rather than an empty one — the distinction `null` versus `[]`.
pub(in crate::analysis) fn body_fields(body: &Value) -> Option<&Container> {
    match body {
        Value::Container(container) => Some(container),
        Value::Tagged { container, .. } => Some(container),
        Value::Scalar(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    const FORBIDDEN: [&str; 4] = ["jomini", "ParsedFile", "ParseFault", "EvidenceQuality"];

    /// Every line of `source` that names the file-level parse model outside a comment.
    fn parse_model_references(source: &str) -> Vec<String> {
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

    /// The file-level parse model must not reach the resolver's answer.
    ///
    /// A text scan, like the parser's own gate, and for the same reason: the types involved
    /// are all `pub(in crate::analysis)`, so the compiler cannot tell "the resolver's output"
    /// from "another part of `analysis`". What it can be made to notice is a *name*.
    ///
    /// Scoped to the shipped portion of the file. This module names the forbidden strings in
    /// order to look for them, and a gate that tripped over its own detector would have to be
    /// weakened until it detected nothing.
    #[test]
    fn resolved_output_names_no_parse() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("analysis")
            .join("resolver")
            .join("resolved.rs");
        let source = fs::read_to_string(&path).unwrap();
        let shipped = source
            .split_once("#[cfg(test)]")
            .map_or(source.as_str(), |(shipped, _)| shipped);
        assert!(
            shipped.len() < source.len(),
            "the test module marker moved, so the scan silently covered the whole file"
        );
        let found = parse_model_references(shipped);
        assert!(found.is_empty(), "{}: {}", path.display(), found.join(", "));
    }

    #[test]
    fn the_scan_detects_a_seeded_file_model_reference() {
        // The negative control, run through the same function the gate uses rather than
        // asserted about a string, so a scan that stopped working could not stay green here.
        // Seeded in memory rather than by editing the file, so it runs in ordinary CI.
        let seeded = concat!(
            "pub struct Answer {\n",
            "    // a name in a comment does not count\n",
            "    read: ParsedFile,\n",
            "}\n"
        );
        assert_eq!(parse_model_references(seeded), ["line 3: ParsedFile"]);
    }
}
