//! The Resolution Profile itself: its version, the game build it was established against,
//! and the rows it declares.
//!
//! # The version's change protocol
//!
//! [`RESOLUTION_PROFILE_VERSION`] is the `resolution_profile` component of the analysis
//! version vector, homed here rather than as a literal in
//! [`AnalysisVersionVector`](crate::analysis::version::AnalysisVersionVector) for the same
//! reason `ENUMERATION_POLICY_VERSION` is homed in `source::policy`: a version that lived
//! away from the policy it names could be forgotten in the commit that changed the policy.
//!
//! Bump it when any row's policy changes meaning, when a cell moves between
//! `Resolved` and `Pending`, or when stream construction changes — then re-pin
//! `analysis::version::tests::pinned_current_digest`. Never the re-pin alone.
//!
//! # The game build, and why a record can block a run
//!
//! Every resolved cell in the profile traces to an oracle record captured against one
//! Stellaris build. "Oracle evidence is re-run whenever the supported Stellaris build
//! changes. A changed result blocks the version update until the Resolution Profile and
//! golden expectations are intentionally revised" (docs/technical-design.md, "Resolver
//! contract and game oracle"). [`SUPPORTED_STELLARIS_BUILD`] is what makes that a mechanism:
//! the oracle expectation suite compares it against the build recorded in every consumed
//! record, and a re-capture under a new build fails until this constant, the expectations,
//! and the profile version are all revised together.

use super::constants;
use super::registry::{
    CellStatus, CrossSourceRule, DefinitionReader, FieldRule, KeyRule, NO_REFERENCES, OrderingRule,
    ProvenanceRule, ReferenceHandling, ReferenceRule, RegistryPolicy, RepeatRule, Replacement,
    ShadowUnit, StreamScope,
};
use super::resolved::{FactKind, ReferenceKind};
use super::stream::{ContentFamily, FileScope};

/// The `resolution_profile` component of the analysis version vector.
///
/// - 1: the Phase 4D core — common file selection, per-family semantic streams, the repeat
///   rule, provenance, and visible refusal. No registry row is declared yet.
/// - 2: the technologies row, and a per-kind references cell the engine honours by detecting
///   scripted-constant and inline-script references without resolving them.
/// - 3 (Phase 4F, STE-27): `scripted-triggers`, `scripted-effects`, and `scripted-constants`
///   declared; the technologies row's `ScriptedConstant` entry flips from
///   `DetectedNotResolved` to `ResolvedAgainstConstants`; the reference cell becomes per-kind
///   `CellStatus`, adding the `Parameter` kind (`Pending` everywhere it can appear); and the
///   scripted-constants row's cross-source cell is `Pending`.
pub(in crate::analysis) const RESOLUTION_PROFILE_VERSION: u32 = 3;

/// The Stellaris build every oracle record behind this profile was captured against.
///
/// Read from `docs/spikes/oracle-records/*/manifest.json`, which is the authority; these
/// constants are the *claim* the expectation suite checks that authority against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StellarisBuild {
    pub version: &'static str,
    pub raw_version: &'static str,
    pub mods_compatibility_version: &'static str,
}

pub(super) const SUPPORTED_STELLARIS_BUILD: StellarisBuild = StellarisBuild {
    version: "Pegasus v4.4.6 (fdde)",
    raw_version: "v4.4.6",
    mods_compatibility_version: "4.4",
};

/// Technologies: the first declared row, and the design's first mandatory oracle case.
///
/// Every cell traces to the resolver evaluation's technologies row
/// (`docs/spikes/resolver-evaluation.md`, "Resolution matrix"):
///
/// - **Key and shadow unit.** The block name, shadowed by the common file and directory
///   rules. Technologies declare no inner identifier the way ship components do. What the
///   reader does *not* return is a file-local scripted-constant declaration: vanilla's
///   `00_fallen_empire_tech.txt` opens with `@EnigmaticEngineeringDraw = 0.025`, and reading
///   that as a definition would publish a technology the game does not have
///   ([`top_level_definitions`](super::registry::top_level_definitions)).
/// - **Stream.** `common/technology/*.txt` in the script family's one global path order.
///   Non-recursive: vanilla ships the directory flat and no record settles what a
///   subdirectory under it would do, so this is the measured shape rather than a guess about
///   an unmeasured one. A mod that ships one is a reason to capture a record, not to widen
///   this quietly.
/// - **Duplicates and cross-source.** Last in enumeration order wins, and a repeat spanning
///   both contributors is decided the same way — by position, never by layer. Both directions
///   are evidence: the mod wins from a late-sorting name (`r1`) and loses from an early one
///   (`r10`).
/// - **Fields.** Whole-object replacement. A redefinition that omits `potential` produces a
///   definition with no `potential`, genuinely absent rather than inherited from the one it
///   displaced — the subject drew and the matched control that kept `potential` did not
///   (`r1`). No defaults: no record establishes a value the game supplies when nobody states
///   one, and inventing one would put a fact in a definition that no source and no evidence
///   stands behind.
/// - **References.** `@constant` references resolve against the scripted-constants
///   environment (Phase 4F, STE-27): `r1` proved the game resolves a vanilla constant read
///   from a mod file, so a technology's `cost = @tier5cost3` now carries a
///   [`ConstantFact`](super::resolved::ConstantFact) naming the resolved value and its
///   declaration site, or a typed [`UnresolvedConstant`](super::resolved::UnresolvedConstant)
///   when it does not resolve — never a guessed number, and never silent. `inline_script`
///   inclusions still belong to their own row and own ticket, so that kind stays
///   `DetectedNotResolved`: an effective field carrying one records a `ReferenceFact` saying
///   the value is not final, the same as before Phase 4F.
/// - **Provenance.** The four kinds the matrix names. `Inherited` is deliberately absent:
///   whole-object replacement never produces one, and an undeclared kind is a refusal, so
///   this is the field rule's claim stated where the engine can catch it breaking.
const TECHNOLOGIES: RegistryPolicy = RegistryPolicy {
    name: "technologies",
    key: CellStatus::Resolved(KeyRule {
        reader: DefinitionReader::TopLevelDefinitions,
        shadow: ShadowUnit::CommonFileSelection,
    }),
    stream: CellStatus::Resolved(StreamScope {
        family: ContentFamily::Script,
        scope: FileScope {
            directory: "common/technology",
            extensions: &["txt"],
            recursive: false,
        },
    }),
    duplicates: CellStatus::Resolved(RepeatRule::ReplaceOnRepeat),
    cross_source: CellStatus::Resolved(CrossSourceRule::DecidedByStreamPosition),
    fields: CellStatus::Resolved(FieldRule {
        replacement: Replacement::WholeObject,
        defaults: &[],
    }),
    ordering: CellStatus::Resolved(OrderingRule::SourceOrderPreserved),
    references: CellStatus::Resolved(ReferenceRule {
        kinds: &[
            (
                ReferenceKind::ScriptedConstant,
                CellStatus::Resolved(ReferenceHandling::ResolvedAgainstConstants),
            ),
            (
                ReferenceKind::InlineScript,
                CellStatus::Resolved(ReferenceHandling::DetectedNotResolved),
            ),
        ],
    }),
    provenance: CellStatus::Resolved(ProvenanceRule {
        kinds: &[
            FactKind::Contributed,
            FactKind::Defaulted,
            FactKind::Duplicate,
            FactKind::Shadowed,
        ],
    }),
};

/// No record measures `$PARAM$` substitution in a trigger or effect body
/// (`docs/spikes/resolver-evaluation.md`: "parameter behavior requires resolver-backed
/// investigation" for both scripted triggers and scripted effects). Shared by both rows so
/// the open cell has one wording rather than two that could drift apart.
const PARAMETER_OPEN: (ReferenceKind, CellStatus<ReferenceHandling>) = (
    ReferenceKind::Parameter,
    CellStatus::Pending {
        reason: "no record measures $PARAM$ substitution in a trigger or effect body",
        oracle_gap: "a capture exercising a parameterised scripted trigger/effect call",
    },
);

/// Scripted triggers: whole-block replacement, decided by the same path order as
/// technologies, with parameter substitution left open.
///
/// Every resolved cell traces to `docs/spikes/resolver-evaluation.md`'s "Scripted triggers"
/// row and the `r1`/`r4` evidence technologies already rests on — the collision and
/// replacement mechanics are one finding shared across every last-wins registry, not a
/// second measurement per row:
///
/// - **Key and shadow unit.** The trigger identifier, shadowed by the common file and
///   directory rules — no inner identifier, the same as technologies.
/// - **Stream.** `common/scripted_triggers/*.txt`, non-recursive: the measured shape, not a
///   guess about a nesting mod (the same reasoning [`TECHNOLOGIES`] states for its own
///   directory).
/// - **Duplicates and cross-source.** Last in enumeration order wins — `r1`'s trigger
///   collisions resolve the same direction as its technology ones, and `r4`'s flip moves the
///   winner with the file rather than the content, confirming position and not layer decides
///   it.
/// - **Fields.** Whole replacement: "the shadowed body never evaluates" (resolution matrix).
///   No defaults — nothing establishes one.
/// - **References.** `@constant` is detected, not resolved: a trigger row consuming scripted
///   constants is unmeasured and belongs to its own evidence, unlike the technologies row
///   `r1` directly measured. `$PARAM$` is [`PARAMETER_OPEN`].
/// - **Provenance.** Contributed, duplicate, and shadowed — a trigger's body is a bare
///   `{ … }` block with no fields distinguishing it from a technology's, so the same three
///   kinds a redefinition can produce apply.
const SCRIPTED_TRIGGERS: RegistryPolicy = RegistryPolicy {
    name: "scripted-triggers",
    key: CellStatus::Resolved(KeyRule {
        reader: DefinitionReader::TopLevelDefinitions,
        shadow: ShadowUnit::CommonFileSelection,
    }),
    stream: CellStatus::Resolved(StreamScope {
        family: ContentFamily::Script,
        scope: FileScope {
            directory: "common/scripted_triggers",
            extensions: &["txt"],
            recursive: false,
        },
    }),
    duplicates: CellStatus::Resolved(RepeatRule::ReplaceOnRepeat),
    cross_source: CellStatus::Resolved(CrossSourceRule::DecidedByStreamPosition),
    fields: CellStatus::Resolved(FieldRule {
        replacement: Replacement::WholeObject,
        defaults: &[],
    }),
    ordering: CellStatus::Resolved(OrderingRule::SourceOrderPreserved),
    references: CellStatus::Resolved(ReferenceRule {
        kinds: &[
            (
                ReferenceKind::ScriptedConstant,
                CellStatus::Resolved(ReferenceHandling::DetectedNotResolved),
            ),
            (
                ReferenceKind::InlineScript,
                CellStatus::Resolved(ReferenceHandling::DetectedNotResolved),
            ),
            PARAMETER_OPEN,
        ],
    }),
    provenance: CellStatus::Resolved(ProvenanceRule {
        kinds: &[
            FactKind::Contributed,
            FactKind::Duplicate,
            FactKind::Shadowed,
        ],
    }),
};

/// Scripted effects: the same shape as [`SCRIPTED_TRIGGERS`], one directory over.
///
/// "Duplicates do not accumulate" (resolution matrix) is the one point of emphasis specific
/// to effects: a redefined effect produces exactly one `Shadowed` definition-level fact, the
/// same as any other whole-object-replacement row, and this row's own evidence (`r1`, `r4`)
/// is what confirms effects follow it rather than merging call sites.
const SCRIPTED_EFFECTS: RegistryPolicy = RegistryPolicy {
    name: "scripted-effects",
    key: CellStatus::Resolved(KeyRule {
        reader: DefinitionReader::TopLevelDefinitions,
        shadow: ShadowUnit::CommonFileSelection,
    }),
    stream: CellStatus::Resolved(StreamScope {
        family: ContentFamily::Script,
        scope: FileScope {
            directory: "common/scripted_effects",
            extensions: &["txt"],
            recursive: false,
        },
    }),
    duplicates: CellStatus::Resolved(RepeatRule::ReplaceOnRepeat),
    cross_source: CellStatus::Resolved(CrossSourceRule::DecidedByStreamPosition),
    fields: CellStatus::Resolved(FieldRule {
        replacement: Replacement::WholeObject,
        defaults: &[],
    }),
    ordering: CellStatus::Resolved(OrderingRule::SourceOrderPreserved),
    references: CellStatus::Resolved(ReferenceRule {
        kinds: &[
            (
                ReferenceKind::ScriptedConstant,
                CellStatus::Resolved(ReferenceHandling::DetectedNotResolved),
            ),
            (
                ReferenceKind::InlineScript,
                CellStatus::Resolved(ReferenceHandling::DetectedNotResolved),
            ),
            PARAMETER_OPEN,
        ],
    }),
    provenance: CellStatus::Resolved(ProvenanceRule {
        kinds: &[
            FactKind::Contributed,
            FactKind::Duplicate,
            FactKind::Shadowed,
        ],
    }),
};

/// Scripted constants: first-wins, with the cross-source cell left open.
///
/// - **Key and shadow unit.** The `@`-prefixed symbol, read by
///   [`constants::constant_declarations`] — the complement of
///   [`top_level_definitions`](super::registry::top_level_definitions), which every other
///   script row uses to *skip* exactly these declarations.
/// - **Stream.** [`constants::SCOPE`]: `common/scripted_variables/*.txt`, non-recursive — the
///   measured shape, restated from [`TECHNOLOGIES`]'s own directory reasoning rather than
///   assumed a second time.
/// - **Duplicates.** Reject on repeat: "redefined within one file" and "redefined across two
///   files in one layer" both resolve first-wins (`r1`, `r4`) — the opposite direction from
///   technologies, triggers, and effects.
/// - **Cross-source.** `Pending`. "This yields a prediction the spike has not tested"
///   (`docs/spikes/resolver-evaluation.md`, "There is no layer precedence"): a Target Mod
///   redefining a vanilla constant should win only from an early-sorting file, exactly as
///   events do, but no record measures it. Settled by the next capture, `r19`: a run
///   redefining a vanilla scripted constant from an early-sorting Target Mod file. Asking
///   this row for itself by name refuses wholesale the moment a repeat spans both sources;
///   a *consumer* reading one contested symbol gets `CrossSourcePending` for that symbol
///   alone while every clean symbol still resolves (`constants::Environment`).
/// - **Fields.** Whole-object, with no defaults — vacuous for a row whose body is always a
///   bare scalar, stated rather than left implicit.
/// - **References.** None: a constant declaration's body is a bare scalar, so the field walk
///   that reference detection runs over never has anything to walk
///   (`registry.rs`, "a body that is not an object states no fields"). This row's own
///   evaluation outcomes are [`ConstantFact`](super::resolved::ConstantFact)s with
///   `field: None`, attached to its own declarations directly rather than found by that walk.
/// - **Provenance.** Contributed, duplicate, and shadowed — the same three a redefinable
///   bare-scalar body can produce.
///
/// The chain evaluator behind this row's evaluation is [`constants`]'s own: a forward
/// reference and a cycle both fail to resolve (`r5-risky-constants`), a definition
/// consuming either carries the same failure rather than a fabricated value
/// (`r7-risky-consumed`), and `0.1 + 0.2` compares exactly equal to `0.3` under
/// [`ExactValue`](crate::canonical::numeric::ExactValue) — never under binary floating point.
const SCRIPTED_CONSTANTS: RegistryPolicy = RegistryPolicy {
    name: "scripted-constants",
    key: CellStatus::Resolved(KeyRule {
        reader: DefinitionReader::ConstantDeclarations,
        shadow: ShadowUnit::CommonFileSelection,
    }),
    stream: CellStatus::Resolved(StreamScope {
        family: ContentFamily::Script,
        scope: constants::SCOPE,
    }),
    duplicates: CellStatus::Resolved(RepeatRule::RejectOnRepeat),
    cross_source: CellStatus::Pending {
        reason: "no record measures a scripted-constant repeat spanning Vanilla and the \
                 Target Mod",
        oracle_gap: "the next capture, r19: a run redefining a vanilla scripted constant \
                     from an early-sorting Target Mod file",
    },
    fields: CellStatus::Resolved(FieldRule {
        replacement: Replacement::WholeObject,
        defaults: &[],
    }),
    ordering: CellStatus::Resolved(OrderingRule::SourceOrderPreserved),
    references: CellStatus::Resolved(NO_REFERENCES),
    provenance: CellStatus::Resolved(ProvenanceRule {
        kinds: &[
            FactKind::Contributed,
            FactKind::Duplicate,
            FactKind::Shadowed,
        ],
    }),
};

/// The registry rows this profile declares.
///
/// One row per ticket, because each is a unit of evidence that deserves its own review
/// (`docs/implementation-plan.md`, "Ticketing", granularity 3). Until a row is declared here,
/// asking for it is [`Refusal::UndeclaredRegistry`](super::registry::Refusal) — which is the
/// design's "a content type may be claimed as supported only when every policy it requires is
/// explicit and oracle-backed", enforced rather than intended.
pub(super) const DECLARED: &[RegistryPolicy] = &[
    TECHNOLOGIES,
    SCRIPTED_TRIGGERS,
    SCRIPTED_EFFECTS,
    SCRIPTED_CONSTANTS,
];

pub(super) fn declared(name: &str) -> Option<&'static RegistryPolicy> {
    DECLARED.iter().find(|policy| policy.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::version::AnalysisVersionVector;
    use crate::source::SourceKind;
    use crate::source::fixture::FixtureCorpus;
    use crate::source::snapshot::SourceSnapshot;

    use super::super::resolve;
    use super::super::resolved::{
        ConstantOutcome, FactSite, ReferenceFact, ReferenceKind, UnresolvedConstant,
    };

    /// One Target Mod technology, resolved through the declared row against an empty base.
    fn only_technology(body: &str) -> SourceSnapshot {
        FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"references\"")
            .with_file("common/technology/zz_references.txt", body.as_bytes())
            .build()
            .expect("a well-formed fixture corpus")
    }

    fn empty_vanilla() -> SourceSnapshot {
        FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file("common/technology/00_empty.txt", b"")
            .build()
            .expect("a well-formed fixture corpus")
    }

    fn references(body: &str) -> Vec<ReferenceFact> {
        let vanilla = empty_vanilla();
        let target = only_technology(body);
        let registry = resolve(&vanilla, &target)
            .registry("technologies")
            .expect("the declared row resolves");
        registry
            .get("tech_references")
            .expect("the key resolves")
            .references
            .clone()
    }

    #[test]
    fn the_version_vector_reads_this_profile_version() {
        // The same coupling `source::policy` has with its enumeration component: one
        // constant, so the version cannot be left behind by the commit that changed the
        // policy it names.
        assert_eq!(
            AnalysisVersionVector::current().resolution_profile,
            RESOLUTION_PROFILE_VERSION
        );
    }

    /// The scripted-constant half of the technologies row's references cell, revised for
    /// Phase 4F (STE-27): the `ScriptedConstant` entry now resolves against the constants
    /// environment rather than merely detecting the reference — the flip this test's
    /// predecessor scheduled. `@tier5cost3` is declared nowhere in this fixture pair, so the
    /// resolved outcome is `Unresolved(UndeclaredSymbol)`, and the field still keeps the
    /// reference text (`EffectiveField.value` never synthesizes a literal).
    #[test]
    fn a_scripted_constant_reference_is_resolved_against_the_constants_environment() {
        let vanilla = empty_vanilla();
        let target = only_technology("tech_references = {\n\tcost = @tier5cost3\n\ttier = 1\n}\n");
        let registry = resolve(&vanilla, &target)
            .registry("technologies")
            .expect("the declared row resolves");
        let definition = registry.get("tech_references").expect("the key resolves");

        assert!(
            definition.references.is_empty(),
            "a resolved scripted-constant reference is a ConstantFact, never a ReferenceFact: \
             {:?}",
            definition.references
        );
        assert_eq!(
            definition.constants,
            [super::super::resolved::ConstantFact {
                symbol: "@tier5cost3".to_owned(),
                field: Some("cost".to_owned()),
                site: FactSite::Stream(definition.position.clone()),
                outcome: ConstantOutcome::Unresolved(UnresolvedConstant::UndeclaredSymbol),
            }]
        );

        // The value is still the reference text, not a resolved number and not a hole.
        assert!(definition.states("cost"));
        assert!(
            definition.states("tier"),
            "the literal fields are unaffected"
        );
    }

    /// The positive twin: `r1` proved the game resolves a vanilla `@` constant read from a
    /// mod file, and this is that case at the resolver seam. The vanilla file declares the
    /// constant under `common/scripted_variables`, and the resolved fact carries the exact
    /// value and names the vanilla file as its declaration site.
    #[test]
    fn a_scripted_constant_vanilla_declares_is_resolved_to_its_exact_value() {
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file(
                "common/scripted_variables/00_base_constants.txt",
                b"@tier5cost3 = 750\n",
            )
            .build()
            .expect("a well-formed fixture corpus");
        let target = only_technology("tech_references = {\n\tcost = @tier5cost3\n\ttier = 1\n}\n");
        let registry = resolve(&vanilla, &target)
            .registry("technologies")
            .expect("the declared row resolves");
        let definition = registry.get("tech_references").expect("the key resolves");

        assert!(definition.references.is_empty());
        assert_eq!(definition.constants.len(), 1, "{:?}", definition.constants);
        let fact = &definition.constants[0];
        assert_eq!(fact.symbol, "@tier5cost3");
        assert_eq!(fact.field.as_deref(), Some("cost"));
        let ConstantOutcome::Resolved { value, declaration } = &fact.outcome else {
            panic!(
                "expected the vanilla constant to resolve: {:?}",
                fact.outcome
            );
        };
        assert_eq!(
            value.value(),
            crate::canonical::numeric::SourceNumber::parse("750").value()
        );
        assert_eq!(
            declaration.source(),
            Some(SourceKind::VanillaContent),
            "the declaration site must name the vanilla file that supplied the value"
        );
    }

    /// The inline-script half. **Scheduled to go red in STE-28 (Phase 4G).**
    ///
    /// Nested inside `weight_modifier`, because that is where `r11` found it and a detector
    /// that only looked at top-level fields would report nothing for every real technology
    /// that uses one.
    #[test]
    fn an_inline_script_reference_is_detected_and_left_unexpanded() {
        let found = references(
            "tech_references = {\n\tweight_modifier = {\n\t\tinline_script = \"oracle/factor\"\n\t}\n}\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, ReferenceKind::InlineScript);
        assert_eq!(
            found[0].field, "weight_modifier",
            "attributed to the effective field a consumer reads, not to the container it sits in"
        );
    }

    /// The control for both pins: the row reports nothing when there is nothing to report.
    #[test]
    fn a_technology_with_only_literal_values_carries_no_references() {
        assert!(references("tech_references = {\n\tcost = 100\n\ttier = 1\n}\n").is_empty());
    }

    #[test]
    fn declared_row_names_are_unique() {
        // Two rows answering to one name would make `declared` return whichever came first,
        // and the profile would have a silent second authority for that registry.
        let mut names: Vec<&str> = DECLARED.iter().map(|policy| policy.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate registry row name");
    }
}
