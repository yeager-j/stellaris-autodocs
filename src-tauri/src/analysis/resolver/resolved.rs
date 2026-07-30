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

use crate::canonical::numeric::SourceNumber;
use crate::canonical::path::LogicalPath;
use crate::source::{SourceBytes, SourceKind};
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

/// A kind of reference a definition body can carry.
///
/// Named here rather than in [`registry`](super::registry) because it is part of what leaves:
/// a consumer reading an effective field needs to know the value is not final. The parser
/// already decides which tokens these are — [`ScalarKind::VariableRef`] and
/// [`ScalarKind::VariableExpr`](super::super::parser::ScalarKind) — so nothing here re-decides
/// what a reference is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::analysis) enum ReferenceKind {
    /// `@name` or `@[ expr ]`. Resolved by the scripted-variables row, whose cross-source
    /// cell is still open (`docs/spikes/resolver-evaluation.md`, registry matrix).
    ScriptedConstant,
    /// An `inline_script` inclusion, expanded textually into the consuming definition before
    /// it registers. Owned by the inline-scripts row (`r11`, `r12`).
    InlineScript,
    /// A whole token of `ScalarKind::Parameter` — `$NAME$`. Two different corpus shapes
    /// answer to it, and each row declares for the shape its stream actually carries. In
    /// trigger and effect bodies it is live substitution machinery, and no record measures a
    /// parameterised call, so those rows declare it `Pending`
    /// (`docs/spikes/resolver-evaluation.md`: "parameter behavior requires resolver-backed
    /// investigation"). In event bodies it is a localization reference written bare in
    /// script (`nomads.605`'s `title = $TRANSMISSION$`), so the events row declares it
    /// `DetectedNotResolved` and Phase 5's localization module interprets it
    /// (`r16-loc-reference`).
    Parameter,
    /// A sprite definition's `sprite_sheet_sprite_type` field. Unlike the generic script
    /// references above, this resolves against the final winners of the sprites registry:
    /// the r17 dependents are the evidence that consulting a shadowed sheet definition
    /// attributes the effective texture to the wrong source.
    SpriteSheet,
}

/// A reference found in an effective definition and deliberately not resolved here.
///
/// Separate from [`FactProvenance`] because it answers a different question. The five
/// [`FactKind`]s say where a fact came from; this says the fact is **incomplete** — the
/// effective value still holds the reference text, and the row that expands it has not run.
/// Folding it into `FactKind` would widen the five-kind vocabulary D-098 names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) struct ReferenceFact {
    pub kind: ReferenceKind,
    /// The effective field the reference was found under. Nested references are attributed to
    /// the field a consumer reads, not to the container they happen to sit in — an
    /// `inline_script` inside `weight_modifier` makes `weight_modifier` the unfinished value.
    pub field: String,
    pub site: FactSite,
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

/// One localization file selected for Phase 5 ingestion.
///
/// This is deliberately a file, not a parsed localization document. The resolver decides
/// which bytes exist and in what order the game reads them; the `localization` module owns
/// interpreting those bytes as `.yml`, merging keys LIOS, and resolving references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct LocalizationFile {
    /// Position in the localization family's semantic stream, from zero.
    pub order: u32,
    pub source: SourceKind,
    pub logical: LogicalPath,
    /// The exact snapshot bytes that source identity and path selected.
    pub bytes: SourceBytes,
}

/// One localization file removed before the effective stream, with its original bytes.
///
/// Phase 5 needs both halves: `provenance` explains why the file lost, while `bytes` let the
/// localization parser enumerate the keys that disappeared with it. Keeping only the path
/// would make those key-level casualties unknowable after file selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct ShadowedLocalizationFile {
    pub provenance: FactProvenance,
    pub bytes: SourceBytes,
}

/// The complete file-level handoff from Phase 4 resolution to Phase 5 localization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct LocalizationFileStream {
    /// Surviving files in game load order.
    pub files: Vec<LocalizationFile>,
    /// Whole-file selection losses in the localization scope, including the losing bytes
    /// Phase 5 must interpret to identify key-level casualties.
    pub shadowed_files: Vec<ShadowedLocalizationFile>,
}

/// Why a scripted-constant symbol does not have a resolved value.
///
/// Two different questions collapse into one enum only where they must: a declaration's own
/// value can fail to resolve regardless of who reads it
/// ([`DeclarationNeverResolves`](Self::DeclarationNeverResolves),
/// [`Expression`](Self::Expression)), file-local scoping can fail in ways only a *consumer*
/// can observe ([`LocalDeclarationFollowsConsumer`](Self::LocalDeclarationFollowsConsumer),
/// [`DuplicateLocalDeclaration`](Self::DuplicateLocalDeclaration)), and the symbol can be
/// missing entirely ([`UndeclaredSymbol`](Self::UndeclaredSymbol)).
///
/// A cross-source repeat is deliberately *not* here: `r19` measured it, and the first
/// declaration in the one global stream wins exactly as a same-source repeat does, so a
/// contested symbol resolves rather than carrying a pending fact (Phase 4L, STE-35; the
/// `CrossSourcePending` variant retired with the cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::analysis) enum UnresolvedConstant {
    /// No declaration of this symbol was found — not locally, not globally.
    UndeclaredSymbol,
    /// The declaration's own value is a `@name` reference that a forward reference or a
    /// cycle prevented from ever settling. The game's own diagnostics do not reliably name
    /// the constant (`r7`'s "unknown command 'tier' for MTTH"), which is why detection is
    /// resolver-owned rather than read off the error log.
    DeclarationNeverResolves,
    /// The declaration's value is a `@[ … ]` expression. Not evaluated — that is a different
    /// and unmeasured question from a plain reference chain.
    Expression,
    /// The consuming file declares this symbol locally, but only after the consumer reads
    /// it. Once any local declaration exists there is no fall-through to the global binding
    /// (decision 8), so a consumer here gets this fact rather than a value from elsewhere.
    LocalDeclarationFollowsConsumer,
    /// The consuming file declares this symbol locally more than once.
    DuplicateLocalDeclaration,
    /// The consuming file's local declaration of this symbol has a reference body. Only a
    /// literal local override is measured (r1); resolving the reference against the global
    /// registry would fall through to the very binding the local shadows (`@cost = @cost`),
    /// and resolving it against other locals is equally unmeasured — so it stays
    /// typed-unresolved rather than either guess.
    LocalReferenceUnmeasured,
}

/// A scripted constant's evaluated chain outcome: its own value, or why it does not have one.
///
/// `Resolved` still carries a [`SourceNumber`] rather than a raw numeric type, because the
/// same non-numeric-lexeme contract `SourceNumber` already states applies here too — `@flag
/// = yes` is a resolved, valueless binding, not a resolver error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) enum ConstantOutcome {
    Resolved {
        value: SourceNumber,
        /// Where the value ultimately came from: the declaration whose own value is a
        /// literal, followed to the end of whatever reference chain led to it.
        declaration: FactSite,
    },
    Unresolved(UnresolvedConstant),
}

/// A fact about one scripted-constant symbol's evaluated outcome.
///
/// Separate from [`ReferenceFact`] because it carries a resolved answer rather than marking
/// a value as still a reference: a consumer with a [`ConstantFact`] on a field learns
/// *whether* `@name` resolved and to what, not merely that the field is not final yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct ConstantFact {
    pub symbol: String,
    /// The consuming field this fact is about, or `None` when it is about a constant
    /// declaration's own value rather than something that read it.
    pub field: Option<String>,
    pub site: FactSite,
    pub outcome: ConstantOutcome,
}

/// Why an `inline_script` inclusion did not expand.
///
/// Every variant is a *typed absence*: the inclusion is omitted from the effective field and
/// this says why, because "failed to expand" and "there was nothing to expand" are otherwise
/// the same silence — the hazard `r11` and `r12` exist to name. Each variant states the
/// record it rests on, or the gap that would settle it, the same way
/// [`UnresolvedConstant`] does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) enum UnresolvedInline {
    /// No file under `common/inline_scripts` supplies the referenced path. `r12` measured
    /// this directly: the game names the consuming file and line, and registers the
    /// definition anyway with the included content simply absent.
    UnknownPath,
    /// The referenced path is already being expanded further up the same chain. No record
    /// measures a cyclic inclusion, but termination is not optional — a typed refusal is the
    /// only answer here that is not a guess about an unmeasured shape.
    CyclicInclusion,
    /// The call site is neither a scalar path nor a container holding a scalar `script` field
    /// and scalar bindings. Those two forms are what `r11` measured (`inline_script = "path"`
    /// and `inline_script = { script = path  F = 1000000 }`); anything else is an unmeasured
    /// call shape rather than a shape this expander may invent a reading for.
    CallShapeUnmeasured,
    /// The fragment uses a `$PARAM$` the call never binds. Substituting an empty value would
    /// fabricate content, and leaving the token in place would put a `Parameter` scalar into
    /// an effective field of a row that does not declare that reference kind — refusing the
    /// whole definition for a fault local to one inclusion. Settled by a capture measuring
    /// what the game does with an unbound parameter.
    UnboundParameter { name: String },
    /// The fragment carries a `[[PARAM] … ]` conditional block. The dialect parses them
    /// (`Item::Conditional`), but no record measures whether — or with what truth condition —
    /// the game compiles one inside an inline script, so the inclusion is omitted rather than
    /// evaluated on a documented-but-unmeasured reading. Settled by a capture exercising a
    /// conditional block in a fragment.
    ConditionalUnmeasured,
    /// The site is a definition's own top-level `inline_script` field rather than one nested
    /// inside a field's value. `r11` measured inclusion sites nested in `weight_modifier`
    /// only, and a root-level site raises a question the record does not answer: what the
    /// spliced fields would even be fields *of*. Settled by a capture placing an inclusion
    /// directly under a definition.
    RootPlacementUnmeasured,
    /// This definition has already expanded as many sites as the expander will spend on one
    /// definition, so the remaining inclusions were omitted.
    ///
    /// The only variant here that names no oracle gap, because no oracle record is implicated:
    /// this is a resolver-owned resource bound on untrusted input, not a claim about anything
    /// the game does. Cycle detection guards the ancestor chain, which is not the same as
    /// bounding the work — a fragment that includes the next one *twice*, nested `k` deep, is
    /// perfectly acyclic and describes 2^k sites in a handful of tiny files. A capture would
    /// settle nothing, since the game's own behaviour on such a corpus is not the question;
    /// the question is that a mod is untrusted input and a build must terminate.
    ExpansionBudgetExceeded,
}

/// What happened at one inline-script expansion site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) enum InlineOutcome {
    Expanded {
        /// The inline script file whose content was spliced in — the "resolved source path"
        /// the resolution matrix requires of every expansion site.
        script: FactSite,
        /// The parameter bindings this call supplied, in the order the call states them.
        /// Recorded whether or not the fragment used them: `r11` bound a parameter its
        /// fragment never referenced, and the game accepted it, so an unused binding is a
        /// fact about the call rather than a fault.
        bindings: Vec<(String, String)>,
    },
    Unresolved(UnresolvedInline),
}

/// One inline-script expansion site, and what became of it.
///
/// Separate from [`ReferenceFact`] for the reason [`ConstantFact`] is: a `ReferenceFact` says
/// an effective value is *not final*, and after Phase 4G an inclusion site always is — it was
/// spliced in, or it was omitted with a typed reason. Neither is an unfinished value waiting
/// for another row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct InlineScriptFact {
    /// The script path the call site named. `None` only when the call named no path at all,
    /// which nothing but [`UnresolvedInline::CallShapeUnmeasured`] can produce — absent
    /// rather than empty, because `script = ""` is a call that named a path and that path is
    /// a different fact from a call with no `script` field.
    pub reference: Option<String>,
    /// The effective field the site was found under. Nested sites are attributed to the same
    /// field their outermost call was, because that is the unit a consumer reads — an
    /// inclusion three fragments deep still makes `weight_modifier` what it makes.
    pub field: String,
    /// Where the call site itself is: the consuming definition's position for a site the
    /// definition states, and the inline script file's own position for a site found inside a
    /// fragment.
    pub site: FactSite,
    pub outcome: InlineOutcome,
}

/// The primary texture a resolved sprite ultimately names.
///
/// The path stays source text at this seam. Phase 8 owns turning it into a normalized Source
/// Asset Reference and reading bytes; the resolver owns only which value won and where that
/// value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct ResolvedSpriteTexture {
    pub path: String,
    pub site: FactSite,
}

/// Whether a sprite has an effective primary texture.
///
/// Missing and cyclic references remain typed outcomes instead of becoming an empty path or
/// a best-effort fallback to a definition the registry did not select.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) enum SpriteTextureOutcome {
    Resolved(ResolvedSpriteTexture),
    MissingTexture,
    MissingTarget {
        sprite: Option<String>,
    },
    /// The effective field is a scalar, but not a quoted or unquoted literal. Its kind is
    /// retained so a scripted constant cannot masquerade as either an asset path or a sprite
    /// registry key.
    UnresolvedScalar {
        kind: super::super::parser::ScalarKind,
    },
    CyclicReference {
        sprite: String,
    },
}

/// One sprite definition's direct `sprite_sheet_sprite_type` edge.
///
/// `target` names the winning referenced definition when one exists. `outcome` is the final
/// value reached through this edge, including that value's own source site, so a Vanilla
/// dependent can truthfully attribute a texture supplied by the Target Mod. Transitive edges
/// remain on their owning definitions instead of being copied onto every upstream sprite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct SpriteReferenceEdge {
    pub sprite: Option<String>,
    pub site: FactSite,
    pub target: Option<FactSite>,
    pub outcome: SpriteTextureOutcome,
}

/// Sprite-specific resolved content attached only to definitions from the sprites row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct SpriteResolution {
    pub texture: SpriteTextureOutcome,
    /// This definition's direct edge, when it has one.
    pub references: Vec<SpriteReferenceEdge>,
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
    /// References this definition carries that the row detected and did not resolve, in
    /// effective-field order. Empty when the row's definitions carry none.
    pub references: Vec<ReferenceFact>,
    /// Scripted-constant evaluation outcomes: one per `@` reference the row resolved against
    /// the constants environment, plus — for the constants row's own definitions — one fact
    /// about the declaration's own value (`field: None`). Empty for a row with no resolving
    /// handling declared.
    pub constants: Vec<ConstantFact>,
    /// Inline-script expansion outcomes: one per site, nested sites included, in the order
    /// expansion reached them. Empty for a row that does not declare
    /// `ExpandedFromInlineScripts`, and for a definition that names no inline script.
    ///
    /// These describe `fields`, not `body`: expansion rewrites the effective view and leaves
    /// the winning definition exactly as parsed, because `body` is what carries the source
    /// ranges Source Excerpts need and a fragment's content has ranges into another file.
    pub inline_expansions: Vec<InlineScriptFact>,
    /// The primary texture and sheet-reference chain for a definition from the sprites row.
    /// `None` for every other registry.
    pub sprite: Option<SpriteResolution>,
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
