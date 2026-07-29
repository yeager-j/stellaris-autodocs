//! The Resolution Profile's registry vocabulary, and the engine that applies one row.
//!
//! # The model this implements
//!
//! Four steps, and only the first mentions a source layer
//! (`docs/spikes/resolver-evaluation.md`, "The remaining rule: accept or reject a repeat"):
//!
//! ```text
//! 1. Resolve same-path file collisions   (Target Mod's file replaces Vanilla's)
//! 2. Enumerate every surviving file in one global normalized-path order
//! 3. Within a file, take definitions in source order
//! 4. On a repeat registration, apply the registry's rule: replace, or reject
//! ```
//!
//! Steps 1 and 2 are [`super::selection`] and [`super::stream`]. Steps 3 and 4 are here. A
//! resolver that ranks sources before paths gets 2 and 4 wrong *in opposite directions* for
//! the two registry groups, which is why nothing below reads [`SourceKind`] to decide a
//! winner — it appears only in provenance and in the one cross-source cell a row must
//! declare.
//!
//! # Why a row is a filled-in struct
//!
//! D-098 requires every row to state eight policies. A struct with eight fields is the
//! earliest reliable mechanism for that: a row that forgets one does not compile. A row that
//! has no evidence for one states [`CellStatus::Pending`], and reaching that cell is a typed
//! [`Refusal`] rather than a borrowed neighbour's answer. This is what keeps the
//! scripted-constants cross-source cell open while its same-source behaviour resolves, and
//! what makes "unsupported content types fail visibly" a mechanism instead of an intention.

use crate::canonical::path::LogicalPath;
use crate::source::SourceKind;
use std::collections::BTreeMap;
use std::fmt;

use super::constants;
use super::resolved::{
    ConstantFact, ConstantOutcome, DefinitionKey, EffectiveField, FactKind, FactProvenance,
    FactSite, ReferenceFact, ReferenceKind, ResolvedDefinition, ResolvedRegistry, StreamPosition,
    UnresolvedConstant, body_fields,
};
use super::selection::FileSelection;
use super::stream::{ContentFamily, FileScope, StreamEntry};

use super::super::parser::{Field, Item, ParsedFile, Scalar, ScalarKind, SourceIdentity, Value};

/// The eight policies D-098 requires of every row.
///
/// A closed set so a refusal can name which one is missing, and so adding a ninth is a
/// deliberate edit to the profile's contract rather than a new string somewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::analysis) enum PolicyCell {
    DefinitionKey,
    FileStream,
    DuplicateWithinStream,
    CrossSourceCollision,
    FieldRule,
    Ordering,
    UnresolvedReferences,
    Provenance,
}

impl fmt::Display for PolicyCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::DefinitionKey => "definition key",
            Self::FileStream => "content-family file stream",
            Self::DuplicateWithinStream => "duplicate within one semantic stream",
            Self::CrossSourceCollision => "cross-source collision",
            Self::FieldRule => "field replacement, inheritance, and defaults",
            Self::Ordering => "ordering of repeated definitions and values",
            Self::UnresolvedReferences => "unresolved references",
            Self::Provenance => "provenance",
        })
    }
}

/// One policy cell: settled by an oracle record, or openly not settled.
///
/// `Pending` is a deliberate blocker. "No registry falls back to a generic last-wins or
/// merge policy" (`docs/spikes/resolver-evaluation.md`, resolution matrix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CellStatus<T> {
    Resolved(T),
    Pending {
        /// What is not known.
        reason: &'static str,
        /// What observation would settle it.
        oracle_gap: &'static str,
    },
}

/// A typed refusal. Never a merge, never a neighbour's policy, never a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) enum Refusal {
    /// No row declares this registry. The MVP supports a content type only when every policy
    /// it needs is explicit and oracle-backed.
    UndeclaredRegistry { registry: String },
    /// The row exists but the resolution reached a cell it cannot answer.
    UnresolvedCell {
        registry: &'static str,
        cell: PolicyCell,
        reason: &'static str,
        oracle_gap: &'static str,
    },
    /// The Target Mod declared a `replace_path` that is not a usable logical path, so the
    /// surviving file set cannot be established. Every registry draws from that set, so no
    /// registry's content can be trusted — the declaration's *intent* is clear (exclude a
    /// directory) and its target is not, and quietly keeping the files it meant to exclude
    /// would produce confidently wrong documentation.
    UnusableReplacePath { declaration: String },
    /// The engine produced a provenance kind the row did not declare it produces. A
    /// contradiction between a row's claim and its behaviour, refused rather than emitted,
    /// because a consumer reading provenance would otherwise be told something no row stands
    /// behind.
    UndeclaredFactKind {
        registry: &'static str,
        kind: FactKind,
    },
    /// A definition carries a kind of reference the row's references cell does not name. The
    /// same contradiction as [`Self::UndeclaredFactKind`], read the other way round: the row
    /// claimed its definitions never carry this, and one does. Skipping it silently would
    /// publish a definition whose value is not final while nothing said so.
    UndeclaredReferenceKind {
        registry: &'static str,
        kind: ReferenceKind,
        key: String,
        field: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndeclaredRegistry { registry } => write!(
                f,
                "no Resolution Profile row declares the registry `{registry}`"
            ),
            Self::UnresolvedCell {
                registry,
                cell,
                reason,
                oracle_gap,
            } => write!(
                f,
                "registry `{registry}` has no policy for its {cell} cell: {reason}. \
                 Settled by: {oracle_gap}"
            ),
            Self::UnusableReplacePath { declaration } => write!(
                f,
                "the Target Mod declared {declaration}, so which files survive cannot be \
                 established and no registry can be resolved"
            ),
            Self::UndeclaredFactKind { registry, kind } => write!(
                f,
                "registry `{registry}` produced a {kind:?} fact its provenance cell does not \
                 declare"
            ),
            Self::UndeclaredReferenceKind {
                registry,
                kind,
                key,
                field,
            } => write!(
                f,
                "registry `{registry}` claims its definitions carry no {kind:?} reference, but \
                 `{key}` carries one in `{field}`"
            ),
        }
    }
}

/// One definition as a row's reader found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReadDefinition {
    pub key: DefinitionKey,
    /// Position within the file, as the parse produced it. Not the index in this vector: a
    /// reader that skips something — [`top_level_definitions`] skips file-local constant
    /// declarations — leaves a gap rather than renumbering, because provenance and Source
    /// Excerpts mean the position in the source.
    pub ordinal: u32,
    pub body: Value,
}

/// How a row finds its definitions inside one parsed file.
///
/// A closed set rather than a function pointer. A function pointer was the first shape this
/// took, and it was wrong: comparing two of them compares addresses, and a linker is free to
/// merge or duplicate identical function bodies — so `is_constants_row`'s recognition of the
/// constants row would go silently false on a build where `constant_declarations` got merged
/// with another function of the same shape, and the row would resolve without ever attaching
/// its own declarations' `ConstantFact`s. An enum decides the distinction once, at the type
/// level, and `resolve` matches on it (never on an address).
///
/// A row hook rather than a fixed rule because the key is not always the block name: sprites
/// live inside a `spriteTypes` block and key on an inner `name`, ship components on an inner
/// `key`. A fixed "top-level field" rule would silently read zero definitions for those. Each
/// variant dispatches to the reader function that already lives with its row —
/// [`top_level_definitions`] here, and
/// [`constants::constant_declarations`](super::constants::constant_declarations) in that
/// module — so a new variant is a one-line addition rather than a reader migrating into this
/// enum's own body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DefinitionReader {
    TopLevelDefinitions,
    ConstantDeclarations,
}

impl DefinitionReader {
    pub(super) fn read(self, file: &ParsedFile) -> Vec<ReadDefinition> {
        match self {
            Self::TopLevelDefinitions => top_level_definitions(file),
            Self::ConstantDeclarations => super::constants::constant_declarations(file),
        }
    }
}

/// Top-level `key = value` definitions, in source order. What most script registries use.
///
/// **A file-local scripted-constant declaration is not a definition of the enclosing
/// registry.** `@EnigmaticEngineeringDraw = 0.025` opens vanilla's
/// `common/technology/00_fallen_empire_tech.txt`, and `00_soc_tech.txt` declares two more
/// mid-file; the evaluation records the construct as "declared in the consuming script file —
/// a file-local declaration overrides the global for that file"
/// (`docs/spikes/resolver-evaluation.md`, "Scripted constants"). A reader that took every
/// top-level field would publish `@EnigmaticEngineeringDraw` as a technology — a definition
/// the game does not have, in a shipped vanilla file, and a silent one: its body is a bare
/// scalar, so it states no effective fields and no reference fact would mark it either.
///
/// Recognized by the key's [`ScalarKind`] rather than by a `@` prefix here, for the reason
/// reference detection reads the same field: the dialect lexer is the authority on what an
/// `@` token is, and a second rule would disagree with it the first time either changed.
///
/// The complement — a reader yielding *only* these declarations — is
/// [`constants::constant_declarations`](super::constants::constant_declarations), which the
/// scripted-constants row's key cell uses.
///
/// Ordinals are the position in the parse, so they are assigned before the skip. A technology
/// following a declaration keeps the ordinal its file gives it; provenance and Source Excerpts
/// mean the position in the source, not the index in this vector.
pub(super) fn top_level_definitions(file: &ParsedFile) -> Vec<ReadDefinition> {
    file.definitions()
        .enumerate()
        .filter(|(_, (field, _))| !is_constant_declaration(field))
        .map(|(ordinal, (field, _))| ReadDefinition {
            key: DefinitionKey::new(field.key.text().into_owned()),
            ordinal: ordinal as u32,
            body: field.value.clone(),
        })
        .collect()
}

/// Whether `field` is a scripted-constant declaration (`@name = …` or `@[ … ] = …`), by the
/// key's [`ScalarKind`] rather than a `@` byte-prefix test. Shared by
/// [`top_level_definitions`], which skips these, and
/// [`constants::constant_declarations`](super::constants::constant_declarations), which
/// yields only these — one predicate, so the two readers cannot disagree about what a
/// declaration is.
pub(super) fn is_constant_declaration(field: &Field) -> bool {
    matches!(
        field.key.kind,
        ScalarKind::VariableRef | ScalarKind::VariableExpr
    )
}

/// The unit at which a row's definitions are shadowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShadowUnit {
    /// The common file and directory rules, unchanged. Every measured row follows them;
    /// a row shown to shadow at some other unit would add a variant here.
    CommonFileSelection,
}

/// `PartialEq` now that [`DefinitionReader`] is a closed enum rather than a function pointer
/// — comparing it compares which variant, not an address. Two *rows* are still the same row
/// because they have the same name, which `profile::declared_row_names_are_unique` enforces;
/// this only lets one `KeyRule` be compared against another when something needs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct KeyRule {
    pub reader: DefinitionReader,
    pub shadow: ShadowUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StreamScope {
    pub family: ContentFamily,
    pub scope: FileScope,
}

/// What a registry does when a key it already holds is registered again.
///
/// The whole of the "remaining rule". Combined with where a mod's filename sorts, it
/// explains every collision result in the spike — and the two directions are why the
/// discriminating r10 pair exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RepeatRule {
    /// The later registration replaces the earlier. Technologies, buildings, megastructures,
    /// scripted triggers, scripted effects. To win, a mod's file must sort **after**.
    ReplaceOnRepeat,
    /// The later registration is rejected. Events, scripted constants. To win, a mod's file
    /// must sort **before**.
    RejectOnRepeat,
}

/// How a repeat that spans two sources is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CrossSourceRule {
    /// By stream position, exactly as a same-source repeat. Cross-source precedence is a
    /// *consequence* of path order plus the repeat rule, never an independent rule — which
    /// is why this is the only resolved variant and why a row that has not been measured
    /// across sources states `Pending` instead of assuming it.
    DecidedByStreamPosition,
}

/// What happens to fields the winning definition does not state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Replacement {
    /// An omitted field is absent. Proven for technologies by the omitted-`potential` case.
    WholeObject,
    /// An omitted field is carried from the definition this one displaced.
    ///
    /// **No shipped row selects this.** It exists because D-098 requires the resolver to be
    /// able to express and record inherited facts, and because the one candidate —
    /// megastructures — has an *inconclusive* oracle cell rather than a negative one. A row
    /// may select it only with an oracle record behind it.
    InheritAbsentFields,
}

/// A field the row supplies when no definition states it.
///
/// `value` is Clausewitz source text, parsed through the one parser at resolution time. A
/// second literal syntax for default values would be a second authority on what a value is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DefaultField {
    pub field: &'static str,
    pub value: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FieldRule {
    pub replacement: Replacement,
    pub defaults: &'static [DefaultField],
}

/// Ordering semantics for repeated definitions and values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OrderingRule {
    /// Source order is preserved and nothing is reordered speculatively
    /// (docs/technical-design.md, "Canonicalization and numeric representation"). The only
    /// order any oracle record exhibits.
    SourceOrderPreserved,
}

/// What a row does with one kind of reference its definitions carry.
///
/// One variant, and it is one the engine honours: a name is offered here only once the code
/// behind it exists. A `Resolved` or `Expanded` the engine did not implement would be a name
/// that lies — a row could select it and get exactly detection, publishing incomplete
/// definitions with a cell that claimed otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReferenceHandling {
    /// Found and recorded as a [`ReferenceFact`]; not expanded. The effective value keeps the
    /// reference text, and the fact is what tells a consumer the value is not final.
    ///
    /// The ticket that implements expansion for a kind changes **that kind's** entry, which
    /// is why this cell is per-kind rather than one verdict for the row: scripted variables
    /// and inline scripts are separate tickets with separate evidence.
    DetectedNotResolved,
    /// Found, looked up in the constants environment, and recorded as a typed
    /// [`ConstantFact`](super::resolved::ConstantFact) — never a guessed value, never a
    /// silent pass-through, and never a [`ReferenceFact`]. Only [`ReferenceKind::ScriptedConstant`]
    /// selects this; the engine builds the environment this handling reads from only when at
    /// least one kind selects it (`registry::resolve`).
    ResolvedAgainstConstants,
}

/// What a row does with the references its definitions carry.
///
/// Per kind, and the list is a claim in both directions. A kind named here is handled as
/// stated — `Pending` included, which defers the claim rather than settling it (decision 6):
/// the engine still holds the row to having *named* the kind, but refuses only once a
/// definition actually carries one, the same lazy consult the cross-source cell uses
/// (`an_open_cross_source_cell_refuses_only_when_a_repeat_spans_two_sources`). A kind *not*
/// named at all is the row's claim that its definitions never carry it — and the engine
/// holds the row to that claim, because a reference nobody declared and nobody recorded is
/// exactly the silent incompleteness this cell exists to prevent
/// ([`Refusal::UndeclaredReferenceKind`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReferenceRule {
    pub kinds: &'static [(ReferenceKind, CellStatus<ReferenceHandling>)],
}

/// A row whose definitions reference nothing that has to resolve here.
pub(super) const NO_REFERENCES: ReferenceRule = ReferenceRule { kinds: &[] };

/// The provenance kinds a row claims to produce.
///
/// Declared rather than derived so the engine can be held to it: producing an undeclared
/// kind is a [`Refusal::UndeclaredFactKind`]. A row that claims `Inherited` while selecting
/// whole-object replacement simply never produces one, which is a harmless
/// over-declaration; the reverse is the failure worth catching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ProvenanceRule {
    pub kinds: &'static [FactKind],
}

/// One row of the Resolution Profile.
#[derive(Debug, Clone, Copy)]
pub(super) struct RegistryPolicy {
    pub name: &'static str,
    pub key: CellStatus<KeyRule>,
    pub stream: CellStatus<StreamScope>,
    pub duplicates: CellStatus<RepeatRule>,
    pub cross_source: CellStatus<CrossSourceRule>,
    pub fields: CellStatus<FieldRule>,
    pub ordering: CellStatus<OrderingRule>,
    pub references: CellStatus<ReferenceRule>,
    pub provenance: CellStatus<ProvenanceRule>,
}

impl RegistryPolicy {
    /// Consults one cell, refusing if it is open.
    fn consult<T: Copy>(&self, cell: PolicyCell, status: &CellStatus<T>) -> Result<T, Refusal> {
        match status {
            CellStatus::Resolved(policy) => Ok(*policy),
            CellStatus::Pending { reason, oracle_gap } => Err(Refusal::UnresolvedCell {
                registry: self.name,
                cell,
                reason,
                oracle_gap,
            }),
        }
    }
}

/// Resolves one registry over the selected files.
///
/// `read` supplies the parse of one stream entry. Passed in rather than taken from a
/// snapshot here so this module never needs to know what a source root is; the caller owns
/// both contributors.
pub(super) fn resolve<R>(
    policy: &RegistryPolicy,
    selection: &FileSelection,
    read: R,
) -> Result<ResolvedRegistry, Refusal>
where
    R: Fn(&StreamEntry) -> Option<ParsedFile>,
{
    // The cells any result at all depends on. Consulted up front so an unusable row refuses
    // before reading a single file, rather than part-way through with half an answer built.
    // Before any cell: a declaration nobody can honour makes the surviving set unknowable,
    // and every cell below is answered over that set.
    if let Some(declaration) = selection.unusable_declarations().first() {
        return Err(Refusal::UnusableReplacePath {
            declaration: declaration.clone(),
        });
    }

    let key = policy.consult(PolicyCell::DefinitionKey, &policy.key)?;
    let scope = policy.consult(PolicyCell::FileStream, &policy.stream)?;
    let repeat = policy.consult(PolicyCell::DuplicateWithinStream, &policy.duplicates)?;
    let fields = policy.consult(PolicyCell::FieldRule, &policy.fields)?;
    let provenance = policy.consult(PolicyCell::Provenance, &policy.provenance)?;
    let reference_rule = policy.consult(PolicyCell::UnresolvedReferences, &policy.references)?;
    // Consulted but not branched on: it has exactly one resolved meaning today, and a row that
    // has not established it must still refuse rather than inherit the default.
    policy.consult(PolicyCell::Ordering, &policy.ordering)?;
    debug_assert!(matches!(key.shadow, ShadowUnit::CommonFileSelection));

    // Whether this row *is* the constants registry, recognized structurally by its key
    // reader variant rather than by name — the same reason `constants::constant_declarations`
    // is recognized by `ScalarKind` rather than a name test. A row's name is the profile's
    // business; the engine only needs to know which reader a row uses.
    let is_constants_row = matches!(key.reader, DefinitionReader::ConstantDeclarations);
    let consumes_constants = reference_rule.kinds.iter().any(|(_, status)| {
        matches!(
            status,
            CellStatus::Resolved(ReferenceHandling::ResolvedAgainstConstants)
        )
    });
    // Built once, before the winners walk (decision 10), and only when something in this
    // resolution actually reads it: a consuming row that resolves scripted constants against
    // it, or the constants row itself attaching a fact to its own declarations.
    let mut environment = (is_constants_row || consumes_constants)
        .then(|| constants::build_environment(selection, &read));

    let stream = super::stream::build(selection, scope.family, scope.scope);
    let mut winners: BTreeMap<DefinitionKey, Winner> = BTreeMap::new();

    for entry in &stream {
        // Unreachable by construction: every stream entry names a surviving file, and a
        // surviving file came from one of the two snapshots' own inventories. Skipping
        // rather than refusing, because a failure here would describe a defect in this
        // module rather than a policy the profile is missing.
        let Some(file) = read(entry) else {
            debug_assert!(false, "a surviving file did not read back: {entry:?}");
            continue;
        };
        if !is_constants_row && let Some(environment) = environment.as_mut() {
            // Collected during this row's own stream walk (decision 10), not a second
            // traversal: a consuming file's local `@` declarations have to be known before
            // `Environment::lookup` can apply the file-local override rule, and this row is
            // already reading the same file's bytes for its own definitions. Skipped for the
            // constants row itself: it never calls `Environment::lookup`, only
            // `declaration_outcome`, which does not consult locals at all.
            let locals: Vec<constants::LocalDeclaration> = constants::constant_declarations(&file)
                .into_iter()
                .map(|declaration| {
                    let site = FactSite::Stream(StreamPosition {
                        order: entry.order,
                        source: entry.source,
                        logical: entry.logical.clone(),
                        ordinal: declaration.ordinal,
                    });
                    constants::LocalDeclaration::new(
                        declaration.key.as_str().to_owned(),
                        declaration.ordinal,
                        declaration.body,
                        site,
                    )
                })
                .collect();
            environment.record_local_declarations(entry.logical.clone(), locals);
        }
        for definition in key.reader.read(&file) {
            let position = StreamPosition {
                order: entry.order,
                source: entry.source,
                logical: entry.logical.clone(),
                ordinal: definition.ordinal,
            };
            match winners.get_mut(&definition.key) {
                None => {
                    winners.insert(
                        definition.key.clone(),
                        Winner::new(position, definition.body),
                    );
                }
                Some(held) => {
                    // A repeat that spans two sources consults the cross-source cell — and
                    // only then. A row measured within one source stays usable for
                    // same-source collisions while its cross-source cell is open, which is
                    // exactly the scripted-constants situation.
                    if held.position.source != position.source {
                        policy.consult(PolicyCell::CrossSourceCollision, &policy.cross_source)?;
                    }
                    held.repeat(repeat, fields.replacement, position, definition.body);
                }
            }
        }
    }

    let mut definitions = winners
        .into_iter()
        .map(|(key, winner)| {
            let definition = winner.into_definition(key.clone(), fields, policy.name);
            (key, definition)
        })
        .collect::<BTreeMap<_, _>>();
    // After the effective fields are settled, not during the walk: a reference in a definition
    // that went on to lose is not a fact about the answer, and recording one would tell a
    // consumer the effective value is unfinished when the value it names never survived.
    for definition in definitions.values_mut() {
        let (references, found_constants) = detect_references(
            definition,
            reference_rule,
            policy.name,
            environment.as_ref(),
        )?;
        definition.references = references;
        definition.constants = found_constants;
    }

    // The constants row's own declarations get a fact about their *own* value (decision 9),
    // separate from `detect_references`: a constant declaration's body is a bare scalar, so
    // it states no effective fields and that walk never runs for it at all.
    if is_constants_row {
        let environment = environment
            .as_ref()
            .expect("built above whenever this row is the constants registry");
        for (key, definition) in definitions.iter_mut() {
            definition.constants = vec![ConstantFact {
                symbol: key.as_str().to_owned(),
                field: None,
                site: FactSite::Stream(definition.position.clone()),
                outcome: environment.declaration_outcome(key.as_str()),
            }];
        }
    }

    let registry = ResolvedRegistry {
        registry: policy.name,
        definitions,
        removed_files: removed_in_scope(selection, scope),
    };
    check_declared_kinds(policy.name, &registry, provenance)?;
    Ok(registry)
}

/// The definition currently holding a key, and everything that lost to it.
///
/// The effective fields are maintained as the stream is walked rather than computed at the
/// end, because inheritance chains: with three registrations of one key, the third inherits
/// from the *effective* second, not from the second's stated fields alone.
struct Winner {
    position: StreamPosition,
    body: Value,
    fields: Vec<EffectiveField>,
    displaced: Vec<FactProvenance>,
}

impl Winner {
    fn new(position: StreamPosition, body: Value) -> Self {
        let fields = stated_fields(&body, &position);
        Self {
            position,
            body,
            fields,
            displaced: Vec::new(),
        }
    }

    /// Applies the row's repeat rule, recording both the duplicate and what it shadowed.
    ///
    /// Both facts, both ways round. "There were two registrations" is true regardless of
    /// which survived, and a consumer that only learned about the survivor could not tell a
    /// clean definition from a contested one.
    fn repeat(
        &mut self,
        rule: RepeatRule,
        replacement: Replacement,
        position: StreamPosition,
        body: Value,
    ) {
        self.displaced.push(FactProvenance {
            kind: FactKind::Duplicate,
            field: None,
            site: FactSite::Stream(position.clone()),
        });
        let (loser, lost_body) = match rule {
            RepeatRule::ReplaceOnRepeat => {
                let displaced = std::mem::replace(&mut self.position, position);
                let mut fields = stated_fields(&body, &self.position);
                if let Replacement::InheritAbsentFields = replacement {
                    carry_absent(&mut fields, &self.fields);
                }
                let lost = std::mem::replace(&mut self.body, body);
                self.fields = fields;
                (displaced, lost)
            }
            RepeatRule::RejectOnRepeat => (position, body),
        };
        // The definition lost, and so did each field it stated. Both, because they answer
        // different questions: the definition-level fact is the only one a body with no
        // fields can produce — a scripted constant's value is a bare scalar — and the
        // field-level facts are what let documentation say *what* a redefinition removed.
        // The resolver evaluation asks the technologies row for "every contributed,
        // defaulted, duplicate, and shadowed **field**", not merely every shadowed
        // definition.
        self.displaced.push(FactProvenance {
            kind: FactKind::Shadowed,
            field: None,
            site: FactSite::Stream(loser.clone()),
        });
        for field in stated_fields(&lost_body, &loser) {
            self.displaced.push(FactProvenance {
                kind: FactKind::Shadowed,
                field: Some(field.field),
                site: field.site,
            });
        }
    }

    fn into_definition(
        mut self,
        key: DefinitionKey,
        rule: FieldRule,
        registry: &'static str,
    ) -> ResolvedDefinition {
        for default in rule.defaults {
            if self.fields.iter().any(|field| field.field == default.field) {
                continue;
            }
            if let Some(value) = parse_default(default) {
                self.fields.push(EffectiveField {
                    field: default.field.to_owned(),
                    value,
                    kind: FactKind::Defaulted,
                    site: FactSite::DeclaredDefault { registry },
                });
            }
        }

        ResolvedDefinition {
            key,
            position: self.position,
            body: self.body,
            fields: self.fields,
            displaced: self.displaced,
            // Attached by `detect_references` once the effective fields are final.
            references: Vec::new(),
            // Attached by `detect_references` (a consuming reference) or, for the constants
            // row's own definitions, by the constants-row post-pass in `resolve`.
            constants: Vec::new(),
        }
    }
}

/// The field name that carries an inline-script inclusion, at any depth.
const INLINE_SCRIPT: &str = "inline_script";

/// Every reference one effective definition carries, or the refusal its row earned.
///
/// One walk decides both. A reference of a declared kind becomes a [`ReferenceFact`]; a
/// reference of a kind the row did not name is [`Refusal::UndeclaredReferenceKind`], because
/// "this row's definitions never carry an inline script" is a claim, and a claim the engine
/// does not check is a comment.
///
/// The walk recurses through containers, tagged values, and conditional blocks, because that
/// is where the references actually are: `r11` found `inline_script` inside `weight_modifier`,
/// not at the top level. Each fact is attributed to the *effective* field it was found under,
/// which is the unit a consumer reads.
///
/// One shape it does not reach, stated because the row that will meet it is already scheduled:
/// a definition whose whole body is a scalar states no fields, so `@foo = @bar` — the
/// scripted-constant shape — produces nothing here. Detection is over effective fields because
/// that is what the technologies row has. A constants row needs its own answer, not a silent
/// zero from this one.
fn detect_references(
    definition: &ResolvedDefinition,
    rule: ReferenceRule,
    registry: &'static str,
    environment: Option<&constants::Environment>,
) -> Result<(Vec<ReferenceFact>, Vec<ConstantFact>), Refusal> {
    let mut scan = Scan {
        definition,
        rule,
        registry,
        environment,
        found: Vec::new(),
        constants: Vec::new(),
    };
    // A definition's own top-level field keys, checked once before the value walk below.
    // `EffectiveField` flattens a key down to its text — exactly what lets a field survive
    // whole-object replacement and a default fill by name — but that flattening is also what
    // erases the key's `ScalarKind`. INLINE_SCRIPT's root check just below can still work off
    // the flattened name because it matches by *text*; `$PARAM$` has to match by *kind*, so
    // this walks the winning body's own container once, where the `Field`'s `Scalar` key is
    // still intact, and attributes the finding to the matching `EffectiveField` by name — a
    // Parameter-keyed field is always a stated field, never a `Defaulted` one, so the match
    // always exists.
    if let Some(container) = body_fields(&definition.body) {
        for body_field in container.fields() {
            if matches!(body_field.key.kind, ScalarKind::Parameter) {
                let Some(effective) = definition
                    .fields
                    .iter()
                    .find(|field| field.field == body_field.key.text())
                else {
                    debug_assert!(
                        false,
                        "a root Parameter key must name a stated effective field: {:?}",
                        body_field.key.text()
                    );
                    continue;
                };
                scan.record(ReferenceKind::Parameter, effective)?;
            }
        }
    }
    for field in &definition.fields {
        if field.field == INLINE_SCRIPT {
            scan.record(ReferenceKind::InlineScript, field)?;
        }
        scan.walk_value(&field.value, field)?;
    }
    Ok((scan.found, scan.constants))
}

/// One definition's walk. The definition, the row's rule, and the registry name are constant
/// for the whole traversal, so they are held once rather than threaded through every frame.
struct Scan<'a> {
    definition: &'a ResolvedDefinition,
    rule: ReferenceRule,
    registry: &'static str,
    /// Present exactly when some kind in `rule` resolves against it
    /// (`ReferenceHandling::ResolvedAgainstConstants`), built once by `resolve` before this
    /// walk runs.
    environment: Option<&'a constants::Environment>,
    found: Vec<ReferenceFact>,
    constants: Vec<ConstantFact>,
}

impl Scan<'_> {
    fn walk_value(&mut self, value: &Value, field: &EffectiveField) -> Result<(), Refusal> {
        match value {
            // The parser already decided which tokens are variable references (`ScalarKind`),
            // so nothing here re-derives it from the raw bytes. A second rule for "what is an
            // `@`" would disagree with the lexer the first time one of them changed.
            Value::Scalar(scalar) => self.walk_scalar(scalar, field)?,
            Value::Container(container) => self.walk_items(&container.items, field)?,
            // The tag is walked too. `rgb { … }` is the shape this exists for and a tag is
            // never a reference today, but skipping a scalar because it is currently
            // uninteresting is how a detector acquires a blind spot.
            Value::Tagged { tag, container, .. } => {
                self.walk_scalar(tag, field)?;
                self.walk_items(&container.items, field)?;
            }
        }
        Ok(())
    }

    fn walk_scalar(&mut self, scalar: &Scalar, field: &EffectiveField) -> Result<(), Refusal> {
        if matches!(
            scalar.kind,
            ScalarKind::VariableRef | ScalarKind::VariableExpr
        ) {
            self.record_scripted_constant(scalar, field)?;
        }
        if matches!(scalar.kind, ScalarKind::Parameter) {
            self.record(ReferenceKind::Parameter, field)?;
        }
        Ok(())
    }

    fn walk_items(&mut self, items: &[Item], field: &EffectiveField) -> Result<(), Refusal> {
        for item in items {
            match item {
                Item::Field(nested) => {
                    if nested.key.text() == INLINE_SCRIPT {
                        self.record(ReferenceKind::InlineScript, field)?;
                    }
                    // A narrow check on the nested field's own KEY, not a general nested-key
                    // walk: a `$PARAM$ = value` key is a real shape (`$PARAM$` substituted
                    // before the block is read), while a nested `@`-key would be an
                    // unmeasured claim this engine has no business making. Only `Parameter`
                    // gets this treatment.
                    if matches!(nested.key.kind, ScalarKind::Parameter) {
                        self.record(ReferenceKind::Parameter, field)?;
                    }
                    self.walk_value(&nested.value, field)?;
                }
                Item::Element(value) => self.walk_value(value, field)?,
                Item::Conditional(conditional) => self.walk_items(&conditional.items, field)?,
            }
        }
        Ok(())
    }

    /// Records a scripted-constant reference: found and left as a [`ReferenceFact`], found
    /// and resolved against the environment as a [`ConstantFact`], deferred by a `Pending`
    /// cell, or refused because the row never declared the kind. Separate from [`Self::record`]
    /// because only this kind ever looks a symbol up, and only this path needs the scalar's
    /// own text to do it.
    fn record_scripted_constant(
        &mut self,
        scalar: &Scalar,
        field: &EffectiveField,
    ) -> Result<(), Refusal> {
        let declared = self
            .rule
            .kinds
            .iter()
            .find(|(declared, _)| *declared == ReferenceKind::ScriptedConstant)
            .map(|(_, status)| *status);
        match declared {
            None => Err(Refusal::UndeclaredReferenceKind {
                registry: self.registry,
                kind: ReferenceKind::ScriptedConstant,
                key: self.definition.key.as_str().to_owned(),
                field: field.field.clone(),
            }),
            Some(CellStatus::Pending { reason, oracle_gap }) => Err(Refusal::UnresolvedCell {
                registry: self.registry,
                cell: PolicyCell::UnresolvedReferences,
                reason,
                oracle_gap,
            }),
            Some(CellStatus::Resolved(ReferenceHandling::DetectedNotResolved)) => {
                self.found.push(ReferenceFact {
                    kind: ReferenceKind::ScriptedConstant,
                    field: field.field.clone(),
                    site: field.site.clone(),
                });
                Ok(())
            }
            Some(CellStatus::Resolved(ReferenceHandling::ResolvedAgainstConstants)) => {
                // A `@[ … ]` expression is not a symbol name, so it is never looked up as
                // though its text were one — that would either miss and claim
                // `UndeclaredSymbol` (false: an expression is not a *symbol* at all, declared
                // or not) or, worse, coincidentally collide with a real declaration. The
                // vocabulary already has `Expression` for exactly this shape.
                let outcome = if let ScalarKind::VariableExpr = scalar.kind {
                    ConstantOutcome::Unresolved(UnresolvedConstant::Expression)
                } else {
                    let environment = self.environment.expect(
                        "resolve() builds the environment whenever a row declares \
                         ResolvedAgainstConstants for any kind",
                    );
                    environment.lookup(
                        &self.definition.position.logical,
                        self.definition.position.ordinal,
                        &scalar.text(),
                    )
                };
                self.constants.push(ConstantFact {
                    symbol: scalar.text().into_owned(),
                    field: Some(field.field.clone()),
                    site: field.site.clone(),
                    outcome,
                });
                Ok(())
            }
        }
    }

    /// Records one detected reference, or refuses because the row did not declare its kind,
    /// or defers because the kind's cell is `Pending`. Used by every kind except
    /// `ScriptedConstant`, which needs the scalar's own text and so goes through
    /// [`Self::record_scripted_constant`] instead.
    ///
    /// The match over [`ReferenceHandling`] is exhaustive on purpose: the ticket that teaches
    /// the engine to expand one of these kinds adds a variant, and this is where it must
    /// decide what to do instead of recording. `ResolvedAgainstConstants` never reaches here
    /// in practice — no shipped row declares it for a kind other than `ScriptedConstant` — and
    /// the `unreachable!` says so rather than silently recording the wrong kind of fact.
    fn record(&mut self, kind: ReferenceKind, field: &EffectiveField) -> Result<(), Refusal> {
        let declared = self
            .rule
            .kinds
            .iter()
            .find(|(declared, _)| *declared == kind)
            .map(|(_, status)| *status);
        match declared {
            None => Err(Refusal::UndeclaredReferenceKind {
                registry: self.registry,
                kind,
                key: self.definition.key.as_str().to_owned(),
                field: field.field.clone(),
            }),
            Some(CellStatus::Pending { reason, oracle_gap }) => Err(Refusal::UnresolvedCell {
                registry: self.registry,
                cell: PolicyCell::UnresolvedReferences,
                reason,
                oracle_gap,
            }),
            Some(CellStatus::Resolved(ReferenceHandling::DetectedNotResolved)) => {
                self.found.push(ReferenceFact {
                    kind,
                    field: field.field.clone(),
                    site: field.site.clone(),
                });
                Ok(())
            }
            Some(CellStatus::Resolved(ReferenceHandling::ResolvedAgainstConstants)) => {
                unreachable!(
                    "only ScriptedConstant routes through record_scripted_constant; no row \
                     declares this handling for {kind:?}"
                )
            }
        }
    }
}

/// Carries every field of `previous` that `fields` does not state, re-kinded as inherited.
///
/// The site stays the displaced definition's: an inherited field's provenance must point at
/// where the value actually came from, or "inherited" would name a fact without naming its
/// origin. Reachable only under [`Replacement::InheritAbsentFields`].
fn carry_absent(fields: &mut Vec<EffectiveField>, previous: &[EffectiveField]) {
    for carried in previous {
        if fields.iter().any(|field| field.field == carried.field) {
            continue;
        }
        fields.push(EffectiveField {
            kind: FactKind::Inherited,
            ..carried.clone()
        });
    }
}

/// The fields a body states, in source order.
///
/// A body that is not an object states no fields — a scripted constant's value is a bare
/// scalar. That is an absence of the question, not an empty answer, and neither case
/// produces a field here.
fn stated_fields(body: &Value, position: &StreamPosition) -> Vec<EffectiveField> {
    body_fields(body)
        .into_iter()
        .flat_map(|container| container.fields())
        .map(|field| EffectiveField {
            field: field.key.text().into_owned(),
            value: field.value.clone(),
            kind: FactKind::Contributed,
            site: FactSite::Stream(position.clone()),
        })
        .collect()
}

fn parse_default(default: &DefaultField) -> Option<Value> {
    let source = format!("{} = {}", default.field, default.value);
    let identity = SourceIdentity::new(
        SourceKind::VanillaContent,
        LogicalPath::parse("resolution-profile/defaults").ok()?,
    );
    let parsed = super::super::parser::parse(identity, source.as_bytes());
    parsed
        .definitions()
        .next()
        .map(|(field, _)| field.value.clone())
}

/// Files removed by common file selection that this registry would otherwise have read.
///
/// This is the r6 and r3 evidence in provenance form: the losing file contributes nothing,
/// and the record of it does not depend on any surviving key mentioning it.
fn removed_in_scope(selection: &FileSelection, scope: StreamScope) -> Vec<FactProvenance> {
    selection
        .removed()
        .iter()
        .filter(|removed| scope.scope.admits(&removed.logical))
        .map(|removed| FactProvenance {
            kind: FactKind::Shadowed,
            field: None,
            site: FactSite::RemovedBySelection {
                source: removed.source,
                logical: removed.logical.clone(),
                removal: removed.removal.clone(),
            },
        })
        .collect()
}

fn check_declared_kinds(
    registry: &'static str,
    resolved: &ResolvedRegistry,
    rule: ProvenanceRule,
) -> Result<(), Refusal> {
    for fact in resolved.provenance() {
        if !rule.kinds.contains(&fact.kind) {
            return Err(Refusal::UndeclaredFactKind {
                registry,
                kind: fact.kind,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceSnapshot;
    use crate::source::fixture::FixtureCorpus;

    use super::super::trial::{self, REPLACE_ON_REPEAT, vanilla};

    /// A late-sorting mod file: it is read *after* every `00_…` vanilla file, so it wins a
    /// replace-on-repeat registry — the mirror of the r10 fixture, and the arrangement in
    /// which a whole-object-versus-inheritance difference is observable at all.
    fn late_mod(body: &str) -> SourceSnapshot {
        FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"late\"")
            .with_file("common/technology/zz_late_tech.txt", body.as_bytes())
            .build()
            .expect("a well-formed fixture corpus")
    }

    fn resolve_with(
        policy: &RegistryPolicy,
        target: &SourceSnapshot,
    ) -> Result<ResolvedRegistry, Refusal> {
        let vanilla = vanilla();
        super::super::resolve(&vanilla, target).resolve_row(policy)
    }

    fn kinds_of(definition: &super::super::resolved::ResolvedDefinition) -> Vec<FactKind> {
        let mut kinds: Vec<FactKind> = definition
            .provenance()
            .into_iter()
            .map(|fact| fact.kind)
            .collect();
        kinds.sort_unstable();
        kinds.dedup();
        kinds
    }

    #[test]
    fn whole_object_replacement_leaves_an_omitted_field_absent() {
        // The omitted-`potential` shape, stated at the engine rather than at a row: the
        // winning definition does not mention it, so the effective definition does not have
        // it. The vanilla definition it displaced did.
        let target = late_mod("tech_contested = { tier = 9 }");
        let registry = resolve_with(&REPLACE_ON_REPEAT, &target).expect("a settled row");
        let contested = registry.get("tech_contested").expect("resolves");

        assert_eq!(contested.position.source, SourceKind::TargetMod);
        assert!(contested.states("tier"));
        assert!(
            !contested.states("potential"),
            "an omitted field was inherited under whole-object replacement"
        );
        assert!(!contested.states("cost"));
    }

    #[test]
    fn inheritance_carries_absent_fields_and_names_where_they_came_from() {
        // The only rule that produces `Inherited`. No shipped row selects it; it exists
        // because D-098 requires the resolver to be able to record an inherited fact, and
        // because the one candidate row — megastructures — has an *inconclusive* oracle
        // cell rather than a negative one.
        const INHERITING: RegistryPolicy = trial::with_fields(
            "trial-inheriting",
            FieldRule {
                replacement: Replacement::InheritAbsentFields,
                defaults: &[],
            },
            &[
                FactKind::Contributed,
                FactKind::Inherited,
                FactKind::Duplicate,
                FactKind::Shadowed,
            ],
        );

        let target = late_mod("tech_contested = { tier = 9 }");
        let registry = resolve_with(&INHERITING, &target).expect("a settled row");
        let contested = registry.get("tech_contested").expect("resolves");

        assert!(
            contested.states("potential"),
            "the absent field was not carried"
        );
        let inherited = contested
            .fields
            .iter()
            .find(|field| field.field == "potential")
            .expect("the carried field");
        assert_eq!(inherited.kind, FactKind::Inherited);
        assert_eq!(
            inherited.site.source(),
            Some(SourceKind::VanillaContent),
            "an inherited fact must point at where the value actually came from"
        );
        // The field the winner does state is still its own.
        assert_eq!(
            contested
                .fields
                .iter()
                .find(|field| field.field == "tier")
                .map(|field| field.kind),
            Some(FactKind::Contributed)
        );
    }

    #[test]
    fn a_declared_default_fills_a_field_no_definition_states() {
        const WITH_DEFAULT: RegistryPolicy = trial::with_fields(
            "trial-defaulted",
            FieldRule {
                replacement: Replacement::WholeObject,
                defaults: &[DefaultField {
                    field: "weight",
                    value: "0",
                }],
            },
            &[
                FactKind::Contributed,
                FactKind::Defaulted,
                FactKind::Duplicate,
                FactKind::Shadowed,
            ],
        );

        let target = late_mod("tech_contested = { tier = 9 }");
        let registry = resolve_with(&WITH_DEFAULT, &target).expect("a settled row");
        let contested = registry.get("tech_contested").expect("resolves");
        let weight = contested
            .fields
            .iter()
            .find(|field| field.field == "weight")
            .expect("the defaulted field");
        assert_eq!(weight.kind, FactKind::Defaulted);

        // A default never overrides a stated value.
        let stating = late_mod("tech_contested = { tier = 9 weight = 42 }");
        let registry = resolve_with(&WITH_DEFAULT, &stating).expect("a settled row");
        let contested = registry.get("tech_contested").expect("resolves");
        assert_eq!(
            contested
                .fields
                .iter()
                .find(|field| field.field == "weight")
                .map(|field| field.kind),
            Some(FactKind::Contributed)
        );
    }

    #[test]
    fn all_five_provenance_kinds_are_producible() {
        // D-098's provenance set, in one resolution. `Shadowed` arrives twice over, from the
        // two different ways a definition can lose: displaced by the repeat rule, and
        // removed with its whole file before any stream existed.
        const EVERY_KIND: RegistryPolicy = trial::with_fields(
            "trial-every-kind",
            FieldRule {
                replacement: Replacement::InheritAbsentFields,
                defaults: &[DefaultField {
                    field: "weight",
                    value: "0",
                }],
            },
            &FactKind::ALL,
        );

        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"every\"")
            .with_file(
                "common/technology/zz_late_tech.txt",
                b"tech_contested = { tier = 9 }",
            )
            // A path collision, so the registry also has a file-level shadow to record.
            .with_file(
                "common/technology/00_collided_tech.txt",
                b"tech_sentinel = { tier = 1 }",
            )
            .build()
            .expect("a well-formed fixture corpus");

        let registry = resolve_with(&EVERY_KIND, &target).expect("a settled row");
        let contested = registry.get("tech_contested").expect("resolves");
        assert_eq!(
            kinds_of(contested),
            vec![
                FactKind::Contributed,
                FactKind::Inherited,
                FactKind::Defaulted,
                FactKind::Duplicate,
                FactKind::Shadowed,
            ],
            "FactKind::ALL is ordered, so this compares the set and the order at once"
        );
        assert!(
            registry
                .removed_files
                .iter()
                .any(|fact| matches!(fact.site, FactSite::RemovedBySelection { .. })),
            "a file removed by selection has no stream position and must say so"
        );
    }

    #[test]
    fn every_fact_either_names_a_source_file_or_says_no_source_supplied_it() {
        // Provenance's other required coordinates — resolution order, source, and ordinal —
        // are unsigned and legitimately zero, so what is checkable over every fact is that it
        // does not claim a file it cannot name. A declared default is the one fact with no
        // source, and saying so is the point: a `Defaulted` fact carrying a stream position
        // would tell a reader that some mod's file supplied a value it never mentioned.
        const WITH_DEFAULT: RegistryPolicy = trial::with_fields(
            "trial-sited",
            FieldRule {
                replacement: Replacement::WholeObject,
                defaults: &[DefaultField {
                    field: "weight",
                    value: "0",
                }],
            },
            &FactKind::ALL,
        );
        let target = late_mod("tech_contested = { tier = 9 }");
        let registry = resolve_with(&WITH_DEFAULT, &target).expect("a settled row");
        let facts = registry.provenance();
        assert!(!facts.is_empty());

        let mut defaults = 0;
        for fact in &facts {
            match &fact.site {
                FactSite::DeclaredDefault { registry } => {
                    assert_eq!(*registry, "trial-sited");
                    assert_eq!(fact.kind, FactKind::Defaulted);
                    assert!(fact.site.source().is_none());
                    assert!(fact.site.logical().is_none());
                    defaults += 1;
                }
                site => {
                    assert!(site.source().is_some(), "{fact:?}");
                    assert!(
                        site.logical().is_some_and(|path| !path.as_str().is_empty()),
                        "{fact:?}"
                    );
                }
            }
        }
        assert_eq!(
            defaults,
            registry.definitions.len(),
            "every definition the row resolved should carry the declared default"
        );
    }

    #[test]
    fn a_shadowed_definition_records_every_field_it_stated() {
        // Definition-level and field-level, because they answer different questions. Without
        // the field-level facts nothing downstream can say *what* a redefinition removed,
        // which is the whole substance of the omitted-`potential` case.
        let target = late_mod("tech_contested = { tier = 9 }");
        let registry = resolve_with(&REPLACE_ON_REPEAT, &target).expect("a settled row");
        let contested = registry.get("tech_contested").expect("resolves");

        let shadowed_fields: Vec<&str> = contested
            .displaced
            .iter()
            .filter(|fact| fact.kind == FactKind::Shadowed)
            .filter_map(|fact| fact.field.as_deref())
            .collect();
        assert_eq!(
            shadowed_fields,
            ["tier", "cost", "category", "potential"],
            "the displaced vanilla definition's fields are the record of what was removed"
        );
        assert_eq!(
            contested
                .displaced
                .iter()
                .filter(|fact| fact.kind == FactKind::Shadowed && fact.field.is_none())
                .count(),
            1,
            "exactly one fact says the definition itself lost"
        );
    }

    #[test]
    fn a_same_source_repeat_follows_the_row_s_rule_in_both_directions() {
        // Two definitions of one key in one file, so nothing about sources is involved and
        // the rule is the only thing deciding.
        let two_in_one_file = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"dup\"")
            .with_file(
                "common/technology/zz_late_tech.txt",
                b"tech_dup = { tier = 1 }\ntech_dup = { tier = 2 }",
            )
            .build()
            .expect("a well-formed fixture corpus");

        let replacing = resolve_with(&REPLACE_ON_REPEAT, &two_in_one_file).expect("a settled row");
        assert_eq!(
            replacing
                .get("tech_dup")
                .expect("resolves")
                .position
                .ordinal,
            1,
            "replace-on-repeat kept the earlier definition"
        );

        const REJECTING: RegistryPolicy = RegistryPolicy {
            name: "trial-same-source-rejecting",
            duplicates: CellStatus::Resolved(RepeatRule::RejectOnRepeat),
            ..REPLACE_ON_REPEAT
        };
        let rejecting = resolve_with(&REJECTING, &two_in_one_file).expect("a settled row");
        assert_eq!(
            rejecting
                .get("tech_dup")
                .expect("resolves")
                .position
                .ordinal,
            0,
            "reject-on-repeat kept the later definition"
        );
    }

    #[test]
    fn an_unconditional_pending_cell_refuses_before_any_file_is_read() {
        const OPEN: RegistryPolicy = RegistryPolicy {
            name: "trial-open-duplicates",
            duplicates: CellStatus::Pending {
                reason: "the winner is not named by any diagnostic",
                oracle_gap: "a runtime observable that distinguishes the two bodies",
            },
            ..REPLACE_ON_REPEAT
        };
        let target = late_mod("tech_contested = { tier = 9 }");
        assert_eq!(
            resolve_with(&OPEN, &target),
            Err(Refusal::UnresolvedCell {
                registry: "trial-open-duplicates",
                cell: PolicyCell::DuplicateWithinStream,
                reason: "the winner is not named by any diagnostic",
                oracle_gap: "a runtime observable that distinguishes the two bodies",
            })
        );
    }

    #[test]
    fn an_open_cross_source_cell_refuses_only_when_a_repeat_spans_two_sources() {
        // The scripted-constants situation, at the engine. Same-source behaviour is settled
        // by `r1`, `r4`, `r5`, and `r7`; cross-source is not. A row in that state must stay
        // usable for what it knows and refuse for what it does not — refusing outright would
        // block the resolved half, and answering anyway would derive a cell from a
        // neighbour, which is what the spike exists to prevent.
        const OPEN_ACROSS: RegistryPolicy = RegistryPolicy {
            name: "trial-open-cross-source",
            cross_source: CellStatus::Pending {
                reason: "no record measures this registry against Vanilla",
                oracle_gap: "a run redefining a vanilla key from an early-sorting mod file",
            },
            ..REPLACE_ON_REPEAT
        };

        // A repeat inside one source only: resolves.
        let same_source = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"same\"")
            .with_file(
                "common/technology/zz_late_tech.txt",
                b"tech_own = { tier = 1 }\ntech_own = { tier = 2 }",
            )
            .build()
            .expect("a well-formed fixture corpus");
        let resolved = resolve_with(&OPEN_ACROSS, &same_source).expect("the settled half");
        assert_eq!(
            resolved.get("tech_own").expect("resolves").position.ordinal,
            1
        );

        // A repeat that crosses sources: refuses, naming the cell.
        let across = late_mod("tech_contested = { tier = 9 }");
        assert_eq!(
            resolve_with(&OPEN_ACROSS, &across),
            Err(Refusal::UnresolvedCell {
                registry: "trial-open-cross-source",
                cell: PolicyCell::CrossSourceCollision,
                reason: "no record measures this registry against Vanilla",
                oracle_gap: "a run redefining a vanilla key from an early-sorting mod file",
            })
        );
    }

    #[test]
    fn a_replace_path_nobody_can_honour_refuses_rather_than_resolving_around_it() {
        // The declaration's *intent* is clear — exclude a directory — and its target is not.
        // Keeping the Vanilla files it meant to exclude would resolve successfully and
        // document content the game would not load, which is the "confidently wrong" outcome
        // visible failure exists to prevent. A malformed descriptor is rare; a wrong answer
        // that looks right is not recoverable downstream.
        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file(
                "descriptor.mod",
                b"name=\"m\"\nreplace_path=\"/common/technology\"",
            )
            .with_file(
                "common/technology/zz_late_tech.txt",
                b"tech_own = { tier = 1 }",
            )
            .build()
            .expect("a well-formed fixture corpus");

        assert_eq!(
            resolve_with(&REPLACE_ON_REPEAT, &target),
            Err(Refusal::UnusableReplacePath {
                declaration: "replace_path=\"/common/technology\": path is absolute or carries \
                              a drive prefix"
                    .to_owned()
            })
        );

        // The negative control for the refusal: the same corpus with a usable declaration
        // resolves, so the refusal is about the declaration rather than about the fixture.
        let usable = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file(
                "descriptor.mod",
                b"name=\"m\"\nreplace_path=\"common/technology\"",
            )
            .with_file(
                "common/technology/zz_late_tech.txt",
                b"tech_own = { tier = 1 }",
            )
            .build()
            .expect("a well-formed fixture corpus");
        let resolved = resolve_with(&REPLACE_ON_REPEAT, &usable).expect("a usable declaration");
        assert_eq!(resolved.keys(), ["tech_own"]);
    }

    #[test]
    fn producing_an_undeclared_provenance_kind_refuses() {
        // A row's provenance cell is a claim about what it emits. The engine is held to it,
        // so a row and its behaviour cannot disagree silently — a consumer reading provenance
        // would otherwise be told something no row stands behind.
        const UNDERDECLARED: RegistryPolicy = trial::with_fields(
            "trial-underdeclared",
            FieldRule {
                replacement: Replacement::WholeObject,
                defaults: &[],
            },
            &[FactKind::Contributed],
        );
        let target = late_mod("tech_contested = { tier = 9 }");
        assert_eq!(
            resolve_with(&UNDERDECLARED, &target),
            Err(Refusal::UndeclaredFactKind {
                registry: "trial-underdeclared",
                kind: FactKind::Duplicate,
            })
        );
    }

    /// A row that says its definitions carry no references, over one that does.
    ///
    /// The claim `kinds: &[]` makes is "never carries this", and the engine holds the row to
    /// it. Skipping the reference quietly would publish `cost = @tier5cost3` as though the
    /// value were literal — confidently wrong documentation, produced by a row that never
    /// claimed to handle it.
    #[test]
    fn a_reference_kind_the_row_did_not_declare_refuses() {
        let carries = late_mod("tech_contested = {\n\tcost = @tier5cost3\n}\n");
        assert_eq!(
            resolve_with(&REPLACE_ON_REPEAT, &carries),
            Err(Refusal::UndeclaredReferenceKind {
                registry: "trial-replace-on-repeat",
                kind: ReferenceKind::ScriptedConstant,
                key: "tech_contested".to_owned(),
                field: "cost".to_owned(),
            })
        );

        let inline = late_mod(
            "tech_contested = {\n\tweight_modifier = {\n\t\tinline_script = \"a/b\"\n\t}\n}\n",
        );
        assert_eq!(
            resolve_with(&REPLACE_ON_REPEAT, &inline),
            Err(Refusal::UndeclaredReferenceKind {
                registry: "trial-replace-on-repeat",
                kind: ReferenceKind::InlineScript,
                key: "tech_contested".to_owned(),
                // The nested inclusion is attributed to the effective field a consumer reads,
                // not to the container it happens to sit in.
                field: "weight_modifier".to_owned(),
            })
        );
    }

    /// The control for the refusal above: it must fire on the reference, not on the corpus.
    ///
    /// `@` inside a token is not a variable reference, and the parser already says so — this
    /// asserts the resolver reads that decision rather than re-deriving one from the bytes. A
    /// detector matching "contains `@`" would refuse here, and the two bodies are one
    /// character apart.
    #[test]
    fn a_definition_carrying_no_reference_resolves_under_the_same_row() {
        let plain = late_mod("tech_contested = {\n\tcost = 20\n\ttag = not@aref\n}\n");
        let resolved = resolve_with(&REPLACE_ON_REPEAT, &plain).expect("no reference, no refusal");
        let definition = resolved.get("tech_contested").expect("the key resolves");
        assert!(definition.references.is_empty());
        assert!(definition.states("tag"));
    }

    /// A file-local constant declaration is not a technology, and does not become one.
    ///
    /// The shape is vanilla's, not invented: `common/technology/00_fallen_empire_tech.txt`
    /// opens with `@EnigmaticEngineeringDraw = 0.025` and a later technology reads it, and
    /// `00_soc_tech.txt` declares two more mid-file. A reader taking every top-level field
    /// publishes `@EnigmaticEngineeringDraw` as a technology the game does not have.
    ///
    /// Three assertions, because the failure is quiet in three different ways: the declaration
    /// must not appear as a key, the technologies around it must still resolve, and the
    /// technology *consuming* it must still carry its reference fact — skipping the
    /// declaration must not also skip the consumption.
    #[test]
    fn a_file_local_constant_declaration_is_not_read_as_a_definition() {
        let declaring = late_mod(concat!(
            "@late_draw = 0.025\n",
            "tech_contested = {\n\tcost = 20\n\tweight = @late_draw\n}\n",
            "tech_after_declaration = {\n\tcost = 30\n}\n",
        ));
        let resolved = resolve_with(&trial::TECHNOLOGY_DETECTING_REFERENCES, &declaring)
            .expect("a declaration is skipped, not refused");

        assert!(
            !resolved.keys().contains(&"@late_draw"),
            "a scripted-constant declaration was published as a technology: {:?}",
            resolved.keys()
        );
        assert!(resolved.get("tech_after_declaration").is_some());

        let contested = resolved.get("tech_contested").expect("the key resolves");
        assert_eq!(contested.position.source, SourceKind::TargetMod);
        assert_eq!(
            contested
                .references
                .iter()
                .map(|fact| (fact.kind, fact.field.as_str()))
                .collect::<Vec<_>>(),
            [(ReferenceKind::ScriptedConstant, "weight")],
            "the declaration is skipped; the consumption is still recorded"
        );
    }

    /// The ordinal is the position in the file, not the index among what the reader kept.
    ///
    /// Stated on its own because it is invisible in every other assertion: renumbering would
    /// give `tech_contested` ordinal 0 here and 0 in a file with no declaration, so two
    /// different source positions would report the same provenance.
    #[test]
    fn skipping_a_declaration_leaves_a_gap_rather_than_renumbering() {
        let declaring = late_mod(concat!(
            "@late_draw = 0.025\n",
            "tech_contested = {\n\tcost = 20\n}\n",
        ));
        let resolved = resolve_with(&REPLACE_ON_REPEAT, &declaring).expect("resolves");
        assert_eq!(
            resolved
                .get("tech_contested")
                .expect("the key resolves")
                .position
                .ordinal,
            1,
            "the technology is the file's second top-level field and says so"
        );
    }

    /// The walk reaches every shape a value can take, not only nested objects.
    ///
    /// A tagged container (`rgb { … }`) and a bare array element are both places a reference
    /// can sit, and neither is a field. A walk that only recursed through `Item::Field` would
    /// report nothing here while the definition plainly carries one.
    #[test]
    fn a_reference_inside_a_tagged_container_is_found() {
        let tagged = late_mod("tech_contested = {\n\ticon = rgb { @oracle_red 0 0 }\n}\n");
        assert_eq!(
            resolve_with(&REPLACE_ON_REPEAT, &tagged),
            Err(Refusal::UndeclaredReferenceKind {
                registry: "trial-replace-on-repeat",
                kind: ReferenceKind::ScriptedConstant,
                key: "tech_contested".to_owned(),
                field: "icon".to_owned(),
            })
        );
    }

    /// A definition that lost does not make the answer unfinished.
    ///
    /// Detection runs over effective fields, after the repeat rule has decided. A reference in
    /// a shadowed body names a value that never reached the answer, so recording it would tell
    /// a consumer the effective definition is incomplete when it is not — and here it would
    /// refuse a row whose surviving definitions are entirely literal.
    #[test]
    fn a_reference_in_a_definition_that_lost_is_not_a_fact_about_the_winner() {
        // `tech_contested` is defined by vanilla's `00_…` file with literal values; this
        // late-sorting file both loses that key to nothing and wins it, so instead invert the
        // rule: the mod's `@`-carrying definition is rejected and vanilla's literal one holds.
        let carries = late_mod("tech_contested = {\n\tcost = @tier5cost3\n}\n");
        let resolved = resolve_with(&trial::REPLACE_SCOPE_REJECTING, &carries)
            .expect("the winning definition carries no reference");
        let definition = resolved.get("tech_contested").expect("the key resolves");
        assert_eq!(definition.position.source, SourceKind::VanillaContent);
        assert!(definition.references.is_empty());
    }

    #[test]
    fn a_refusal_says_what_is_missing_and_what_would_settle_it() {
        // The message is the point of a typed refusal reaching a person: "unsupported" tells
        // nobody which evidence to go and get.
        let refusal = Refusal::UnresolvedCell {
            registry: "scripted-constants",
            cell: PolicyCell::CrossSourceCollision,
            reason: "no record measures this registry against Vanilla",
            oracle_gap: "a run redefining a vanilla constant from an early-sorting file",
        };
        let rendered = refusal.to_string();
        assert!(rendered.contains("scripted-constants"), "{rendered}");
        assert!(rendered.contains("cross-source collision"), "{rendered}");
        assert!(rendered.contains("Settled by:"), "{rendered}");
    }

    /// A `Pending` reference-kind cell refuses lazily, the same precedent the cross-source
    /// cell already established: a row that leaves `Parameter` open still resolves a corpus
    /// that never carries one, and refuses only once a definition actually does.
    #[test]
    fn an_open_reference_kind_cell_refuses_only_when_a_definition_carries_it() {
        let plain = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"plain-trigger\"")
            .with_file(
                "common/scripted_triggers/zz_plain.txt",
                b"trig_only = { always = yes }",
            )
            .build()
            .expect("a well-formed fixture corpus");
        let resolved = resolve_with(&trial::TRIGGERS_DECLARING_PARAMETER, &plain)
            .expect("a parameter-free corpus resolves under an open Parameter cell");
        assert!(resolved.get("trig_only").is_some());

        let parameterized = trial::corpus(SourceKind::TargetMod, trial::PARAMETERIZED);
        assert_eq!(
            resolve_with(&trial::TRIGGERS_DECLARING_PARAMETER, &parameterized),
            Err(Refusal::UnresolvedCell {
                registry: "trial-triggers-declaring-parameter",
                cell: PolicyCell::UnresolvedReferences,
                reason: "no record measures $PARAM$ substitution in a trigger or effect body",
                oracle_gap: "a capture exercising a parameterised scripted trigger/effect call",
            })
        );
    }

    /// The control for the cell above: a row that does not name `Parameter` at all refuses
    /// outright, because "not pending" and "not declared" are different claims.
    #[test]
    fn an_undeclared_parameter_kind_refuses() {
        let parameterized = trial::corpus(SourceKind::TargetMod, trial::PARAMETERIZED);
        assert_eq!(
            resolve_with(&trial::TRIGGERS_NO_REFERENCES, &parameterized),
            Err(Refusal::UndeclaredReferenceKind {
                registry: "trial-triggers-no-references",
                kind: ReferenceKind::Parameter,
                key: "trig_param".to_owned(),
                field: "check_variable".to_owned(),
            })
        );
    }

    /// `$PARAM$` used as a nested field's own *key* — a real shape, not a general nested-key
    /// walk: only `Parameter` gets this treatment (decision 6), because a nested `@`-key would
    /// be an unmeasured claim about scripted constants this engine has no business making.
    #[test]
    fn a_parameter_used_as_a_nested_field_key_is_detected() {
        let target =
            late_mod("tech_contested = {\n\tweight_modifier = {\n\t\t$FACTOR$ = 1\n\t}\n}\n");
        let resolved = resolve_with(&trial::TECHNOLOGY_DETECTING_PARAMETER, &target)
            .expect("a declared Parameter kind resolves");
        let definition = resolved.get("tech_contested").expect("the key resolves");
        assert_eq!(
            definition
                .references
                .iter()
                .map(|fact| (fact.kind, fact.field.as_str()))
                .collect::<Vec<_>>(),
            [(ReferenceKind::Parameter, "weight_modifier")],
            "attributed to the effective field a consumer reads, not the nested key itself"
        );
    }

    /// `$PARAM$` used as a definition's own top-level field key — a real shape, and a
    /// different mechanism from the nested-key check above: `EffectiveField` flattens a key
    /// to its text, which is exactly what lets a field survive whole-object replacement, but
    /// it erases the key's `ScalarKind`. Root detection has to walk the winning body's own
    /// container, where the `Field`'s `Scalar` key is still intact.
    #[test]
    fn a_root_level_parameter_key_refuses_under_an_open_cell() {
        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"root-param\"")
            .with_file(
                "common/scripted_triggers/zz_root_param.txt",
                b"trig_root_param = {\n\t$MODE$ = yes\n}\n",
            )
            .build()
            .expect("a well-formed fixture corpus");
        assert_eq!(
            resolve_with(&trial::TRIGGERS_DECLARING_PARAMETER, &target),
            Err(Refusal::UnresolvedCell {
                registry: "trial-triggers-declaring-parameter",
                cell: PolicyCell::UnresolvedReferences,
                reason: "no record measures $PARAM$ substitution in a trigger or effect body",
                oracle_gap: "a capture exercising a parameterised scripted trigger/effect call",
            })
        );
    }

    /// The positive half: a row that declares `Parameter` records the root-level key as a
    /// `ReferenceFact`, attributed to the field itself (its `EffectiveField` name *is* the
    /// key text, since a root field's name and its key are the same thing).
    #[test]
    fn a_root_level_parameter_key_is_detected_when_declared() {
        let target = late_mod("tech_contested = {\n\t$MODE$ = yes\n}\n");
        let resolved = resolve_with(&trial::TECHNOLOGY_DETECTING_PARAMETER, &target)
            .expect("a declared Parameter kind resolves");
        let definition = resolved.get("tech_contested").expect("the key resolves");
        assert_eq!(
            definition
                .references
                .iter()
                .map(|fact| (fact.kind, fact.field.as_str()))
                .collect::<Vec<_>>(),
            [(ReferenceKind::Parameter, "$MODE$")]
        );
    }

    /// The control: a literal-keyed root field is one character away from the shape above and
    /// must carry nothing.
    #[test]
    fn a_literal_keyed_root_field_carries_no_parameter_reference() {
        let target = late_mod("tech_contested = {\n\tmode = yes\n}\n");
        let resolved =
            resolve_with(&trial::TECHNOLOGY_DETECTING_PARAMETER, &target).expect("resolves");
        let definition = resolved.get("tech_contested").expect("the key resolves");
        assert!(
            definition.references.is_empty(),
            "{:?}",
            definition.references
        );
    }

    /// A row resolving `ScriptedConstant` against the constants environment records a typed
    /// [`ConstantFact`] — resolved with the declaration's site for a known symbol, and a typed
    /// `Unresolved(UndeclaredSymbol)` for a missing one — and never a [`ReferenceFact`].
    #[test]
    fn a_row_resolving_against_constants_records_a_constant_fact_never_a_reference_fact() {
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file("common/scripted_variables/00_known.txt", b"@known = 5\n")
            .build()
            .expect("a well-formed fixture corpus");
        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"resolving\"")
            .with_file(
                "common/technology/zz_resolving.txt",
                b"tech_known = {\n\tcost = @known\n\ttier = 1\n}\n\
                  tech_missing = {\n\tcost = @missing\n\ttier = 1\n}\n",
            )
            .build()
            .expect("a well-formed fixture corpus");

        let resolution = super::super::resolve(&vanilla, &target);
        let resolved = resolution
            .resolve_row(&trial::TECHNOLOGY_RESOLVING_CONSTANTS)
            .expect("a settled row");

        let known = resolved.get("tech_known").expect("resolves");
        assert!(
            known.references.is_empty(),
            "never a ReferenceFact: {:?}",
            known.references
        );
        assert_eq!(known.constants.len(), 1, "{:?}", known.constants);
        let fact = &known.constants[0];
        assert_eq!(fact.symbol, "@known");
        assert_eq!(fact.field.as_deref(), Some("cost"));
        let ConstantOutcome::Resolved { value, declaration } = &fact.outcome else {
            panic!("expected @known to resolve: {:?}", fact.outcome);
        };
        assert_eq!(
            value.value(),
            crate::canonical::numeric::SourceNumber::parse("5").value()
        );
        assert_eq!(declaration.source(), Some(SourceKind::VanillaContent));

        let missing = resolved.get("tech_missing").expect("resolves");
        assert_eq!(
            missing.constants,
            vec![ConstantFact {
                symbol: "@missing".to_owned(),
                field: Some("cost".to_owned()),
                site: FactSite::Stream(missing.position.clone()),
                outcome: ConstantOutcome::Unresolved(UnresolvedConstant::UndeclaredSymbol),
            }]
        );
    }

    /// A `@[ … ]` expression consumed by a `ResolvedAgainstConstants` row is never looked up
    /// as though its whole text were a symbol name — it carries `Unresolved(Expression)`
    /// directly. `UndeclaredSymbol` claims "no declaration of this symbol was found", which
    /// would be false here: an expression is not a symbol at all, declared or not.
    #[test]
    fn a_variable_expression_resolved_against_constants_is_never_an_undeclared_symbol() {
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .build()
            .expect("an empty fixture corpus");
        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"expression\"")
            .with_file(
                "common/technology/zz_expression.txt",
                b"tech_expression = {\n\tcost = @[ 1 + 1 ]\n\ttier = 1\n}\n",
            )
            .build()
            .expect("a well-formed fixture corpus");

        let resolution = super::super::resolve(&vanilla, &target);
        let resolved = resolution
            .resolve_row(&trial::TECHNOLOGY_RESOLVING_CONSTANTS)
            .expect("a settled row");
        let definition = resolved.get("tech_expression").expect("resolves");

        assert!(
            definition.references.is_empty(),
            "never a ReferenceFact: {:?}",
            definition.references
        );
        assert_eq!(definition.constants.len(), 1, "{:?}", definition.constants);
        let fact = &definition.constants[0];
        assert_eq!(fact.field.as_deref(), Some("cost"));
        assert_eq!(
            fact.outcome,
            ConstantOutcome::Unresolved(UnresolvedConstant::Expression),
            "an expression is not a symbol, declared or not"
        );
    }

    /// The consuming-side half of the alias-propagation fix
    /// (`constants::tests::cross_source_invalidation_propagates_through_an_alias` is the
    /// required unit test): a technology reading `@alias` must see the same
    /// `CrossSourcePending` its dependency `@base` carries, not the value `@alias` copied
    /// before `@base`'s cross-source collision was known.
    #[test]
    fn a_consumer_reading_an_alias_of_a_contested_symbol_is_pending() {
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file(
                "common/scripted_variables/00_alias.txt",
                b"@base = 5\n@alias = @base\n",
            )
            .build()
            .expect("a well-formed fixture corpus");
        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"alias-consumer\"")
            .with_file("common/scripted_variables/zz_alias.txt", b"@base = 99\n")
            .with_file(
                "common/technology/zz_alias_consumer.txt",
                b"tech_alias_consumer = {\n\tcost = @alias\n\ttier = 1\n}\n",
            )
            .build()
            .expect("a well-formed fixture corpus");

        let resolution = super::super::resolve(&vanilla, &target);
        let resolved = resolution
            .resolve_row(&trial::TECHNOLOGY_RESOLVING_CONSTANTS)
            .expect("a settled row");
        let definition = resolved.get("tech_alias_consumer").expect("resolves");
        let fact = definition.constants.first().expect("a constant fact");
        assert_eq!(
            fact.outcome,
            ConstantOutcome::Unresolved(UnresolvedConstant::CrossSourcePending),
            "a consumer of the alias must not see a value derived from a pending dependency"
        );
    }
}
