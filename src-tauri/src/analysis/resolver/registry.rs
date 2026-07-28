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

use super::resolved::{
    DefinitionKey, EffectiveField, FactKind, FactProvenance, FactSite, ResolvedDefinition,
    ResolvedRegistry, StreamPosition, body_fields,
};
use super::selection::FileSelection;
use super::stream::{ContentFamily, FileScope, StreamEntry};

use super::super::parser::{ParsedFile, SourceIdentity, Value};

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
    /// The engine produced a provenance kind the row did not declare it produces. A
    /// contradiction between a row's claim and its behaviour, refused rather than emitted,
    /// because a consumer reading provenance would otherwise be told something no row stands
    /// behind.
    UndeclaredFactKind {
        registry: &'static str,
        kind: FactKind,
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
            Self::UndeclaredFactKind { registry, kind } => write!(
                f,
                "registry `{registry}` produced a {kind:?} fact its provenance cell does not \
                 declare"
            ),
        }
    }
}

/// One definition as a row's reader found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReadDefinition {
    pub key: DefinitionKey,
    /// Ordinal within the file, in the order the reader yields definitions.
    pub ordinal: u32,
    pub body: Value,
}

/// How a row finds its definitions inside one parsed file.
///
/// A row hook rather than a fixed rule because the key is not always the block name: sprites
/// live inside a `spriteTypes` block and key on an inner `name`, ship components on an inner
/// `key`. A fixed "top-level field" rule would silently read zero definitions for those.
pub(super) type DefinitionReader = fn(&ParsedFile) -> Vec<ReadDefinition>;

/// Top-level `key = value` definitions, in source order. What most script registries use.
pub(super) fn top_level_definitions(file: &ParsedFile) -> Vec<ReadDefinition> {
    file.definitions()
        .enumerate()
        .map(|(ordinal, (field, _))| ReadDefinition {
            key: DefinitionKey::new(field.key.text().into_owned()),
            ordinal: ordinal as u32,
            body: field.value.clone(),
        })
        .collect()
}

/// The unit at which a row's definitions are shadowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShadowUnit {
    /// The common file and directory rules, unchanged. Every measured row follows them;
    /// a row shown to shadow at some other unit would add a variant here.
    CommonFileSelection,
}

/// Deliberately not `PartialEq`: it holds a function pointer, and comparing those compares
/// addresses that a linker is free to merge or duplicate. Two rows are the same row because
/// they have the same name, which `profile::declared_row_names_are_unique` enforces.
#[derive(Debug, Clone, Copy)]
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

/// What a row does with a reference it cannot resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReferenceRule {
    /// The row's definitions reference nothing that has to resolve here.
    NoReferences,
    /// An unresolved reference is retained as a visible unresolved fact rather than dropped.
    VisiblyUnresolved,
}

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
    let key = policy.consult(PolicyCell::DefinitionKey, &policy.key)?;
    let scope = policy.consult(PolicyCell::FileStream, &policy.stream)?;
    let repeat = policy.consult(PolicyCell::DuplicateWithinStream, &policy.duplicates)?;
    let fields = policy.consult(PolicyCell::FieldRule, &policy.fields)?;
    let provenance = policy.consult(PolicyCell::Provenance, &policy.provenance)?;
    // Consulted but not branched on: both have exactly one resolved meaning today, and a row
    // that has not established them must still refuse rather than inherit the default.
    policy.consult(PolicyCell::Ordering, &policy.ordering)?;
    policy.consult(PolicyCell::UnresolvedReferences, &policy.references)?;
    debug_assert!(matches!(key.shadow, ShadowUnit::CommonFileSelection));

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
        for definition in (key.reader)(&file) {
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

    let definitions = winners
        .into_iter()
        .map(|(key, winner)| {
            let definition = winner.into_definition(key.clone(), fields);
            (key, definition)
        })
        .collect::<BTreeMap<_, _>>();

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
        let loser = match rule {
            RepeatRule::ReplaceOnRepeat => {
                let displaced = std::mem::replace(&mut self.position, position);
                let mut fields = stated_fields(&body, &self.position);
                if let Replacement::InheritAbsentFields = replacement {
                    carry_absent(&mut fields, &self.fields);
                }
                self.fields = fields;
                self.body = body;
                displaced
            }
            RepeatRule::RejectOnRepeat => position,
        };
        self.displaced.push(FactProvenance {
            kind: FactKind::Shadowed,
            field: None,
            site: FactSite::Stream(loser),
        });
    }

    fn into_definition(mut self, key: DefinitionKey, rule: FieldRule) -> ResolvedDefinition {
        for default in rule.defaults {
            if self.fields.iter().any(|field| field.field == default.field) {
                continue;
            }
            if let Some(value) = parse_default(default) {
                self.fields.push(EffectiveField {
                    field: default.field.to_owned(),
                    value,
                    kind: FactKind::Defaulted,
                    site: FactSite::Stream(self.position.clone()),
                });
            }
        }

        ResolvedDefinition {
            key,
            position: self.position,
            body: self.body,
            fields: self.fields,
            displaced: self.displaced,
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
            SourceKind::VanillaContent,
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
    fn every_fact_names_the_file_it_came_from() {
        // Provenance's other required coordinates — resolution order, source, and ordinal —
        // are unsigned and legitimately zero, so the checkable claim over every fact is that
        // each one names a real file rather than an empty placeholder.
        let target = late_mod("tech_contested = { tier = 9 }");
        let registry = resolve_with(&REPLACE_ON_REPEAT, &target).expect("a settled row");
        let facts = registry.provenance();
        assert!(!facts.is_empty());
        for fact in facts {
            assert!(!fact.site.logical().as_str().is_empty(), "{fact:?}");
        }
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
}
