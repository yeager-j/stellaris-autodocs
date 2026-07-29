//! The corpora and registry rows the core proves itself against.
//!
//! Test-only. The synthetic registry policies remain deliberately separate from the real
//! rows: each real row is a unit of oracle evidence that deserves its own review
//! (`docs/implementation-plan.md`, "Ticketing"). The core's original demonstration is
//! narrower: one accept-on-repeat and one reject-on-repeat registry, given the same
//! early-sorting mod file, produce *opposite* winners. Two synthetic rows show that without
//! borrowing any real row's judgment, and the real rows re-assert it with their own evidence.
//!
//! The corpora are committed under `fixtures/resolver/` and read with `include_bytes!` — a
//! compile-time read, so nothing here traverses a filesystem or depends on a host.

use crate::source::SourceKind;
use crate::source::fixture::FixtureCorpus;
use crate::source::snapshot::SourceSnapshot;

use super::constants;
use super::registry::{
    CellStatus, CrossSourceRule, DefinitionReader, FieldRule, KeyRule, NO_REFERENCES, OrderingRule,
    ProvenanceRule, ReferenceHandling, ReferenceRule, RegistryPolicy, RepeatRule, Replacement,
    ShadowUnit, StreamScope,
};
use super::resolved::{FactKind, ReferenceKind, ResolvedRegistry};
use super::stream::{ContentFamily, FileScope};

macro_rules! fixture {
    ($path:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../fixtures/resolver/",
            $path
        ))
    };
}

/// The stand-in base-game file set. Everything sorts as `00_…`, so an `!!!_…` mod file is
/// read before all of it and a `zz_…` one after all of it.
pub(super) const VANILLA: &[(&str, &[u8])] = &[
    (
        "common/technology/00_baseline_tech.txt",
        fixture!("vanilla/common/technology/00_baseline_tech.txt"),
    ),
    (
        "common/technology/00_collided_tech.txt",
        fixture!("vanilla/common/technology/00_collided_tech.txt"),
    ),
    (
        "events/00_baseline_notices.txt",
        fixture!("vanilla/events/00_baseline_notices.txt"),
    ),
];

/// `r10-loadorder`: one early-sorting filename applied to both registry rules at once.
pub(super) const EARLY_MOD: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("early-mod/descriptor.mod")),
    (
        "common/technology/!!!_early_tech.txt",
        fixture!("early-mod/common/technology/!!!_early_tech.txt"),
    ),
    (
        "events/!!!_early_notices.txt",
        fixture!("early-mod/events/!!!_early_notices.txt"),
    ),
];

/// `r6-pathcollision`: a mod file at a vanilla file's exact logical path.
pub(super) const PATH_COLLISION: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("path-collision/descriptor.mod")),
    (
        "common/technology/00_collided_tech.txt",
        fixture!("path-collision/common/technology/00_collided_tech.txt"),
    ),
];

/// `r3-replace-path`: a directory replacement plus one of the declarer's own files in it.
pub(super) const REPLACE_PATH: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("replace-path/descriptor.mod")),
    (
        "common/technology/zz_replacement_tech.txt",
        fixture!("replace-path/common/technology/zz_replacement_tech.txt"),
    ),
];

/// The stand-in base game golden case 5 redefines. Separate from [`VANILLA`], whose exact key
/// list the r6 and r3 expectations assert.
pub(super) const REDEFINITION_VANILLA: &[(&str, &[u8])] = &[(
    "common/technology/00_redefinition_tech.txt",
    fixture!("redefinition-vanilla/common/technology/00_redefinition_tech.txt"),
)];

/// `r1-target`: a late-sorting mod file redefining vanilla technologies, one of them omitting
/// `potential`.
pub(super) const REDEFINITION: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("redefinition/descriptor.mod")),
    (
        "common/technology/zz_redefinition_tech.txt",
        REDEFINITION_BODY,
    ),
];

/// The same bytes under an early-sorting name, restating `r4-reordered`'s method: identical
/// content, swapped filename, and the winner must move.
pub(super) const REDEFINITION_FLIPPED: &[(&str, &[u8])] = &[
    (
        "descriptor.mod",
        fixture!("redefinition-flipped/descriptor.mod"),
    ),
    (
        "common/technology/!!!_redefinition_tech.txt",
        REDEFINITION_FLIPPED_BODY,
    ),
];

/// The `r0-baseline` shape: a Target Mod that redefines nothing, so the vanilla definitions
/// are read with nothing contesting them. What makes the `r1` result a delta.
pub(super) const NO_REDEFINITION: &[(&str, &[u8])] =
    &[("descriptor.mod", fixture!("redefinition/descriptor.mod"))];

/// Named separately from the tables above so the byte identity the flip depends on can be
/// asserted rather than assumed.
pub(super) const REDEFINITION_BODY: &[u8] =
    fixture!("redefinition/common/technology/zz_redefinition_tech.txt");
pub(super) const REDEFINITION_FLIPPED_BODY: &[u8] =
    fixture!("redefinition-flipped/common/technology/!!!_redefinition_tech.txt");

/// A declared row resolved by name, panicking with the refusal on the caller's behalf.
///
/// Asking by name rather than through `resolve_row` is deliberate wherever a suite's claim is
/// about the *declared* profile: `resolve_row` would prove the weaker thing, that a policy works
/// when handed straight to the engine. Both [`super::oracle`] and [`super::golden`] make claims
/// of the stronger kind, which is why this lives here rather than in either of them.
pub(super) fn named(resolution: &super::Resolution<'_>, registry: &str) -> ResolvedRegistry {
    resolution
        .registry(registry)
        .unwrap_or_else(|refusal| panic!("the declared {registry} row resolves: {refusal}"))
}

pub(super) fn corpus(kind: SourceKind, files: &[(&str, &[u8])]) -> SourceSnapshot {
    files
        .iter()
        .fold(FixtureCorpus::new(kind), |corpus, (logical, bytes)| {
            corpus.with_file(logical, bytes)
        })
        .build()
        .expect("a committed fixture corpus establishes")
}

pub(super) fn vanilla() -> SourceSnapshot {
    corpus(SourceKind::VanillaContent, VANILLA)
}

pub(super) fn redefinition_vanilla() -> SourceSnapshot {
    corpus(SourceKind::VanillaContent, REDEFINITION_VANILLA)
}

// --- Phase 4F: scripted triggers, effects, and constants ---

/// The base-game scripted constants Phase 4F's corpora consume: `@base_cost` (a clean,
/// cross-source read) and `@shared_symbol` (redeclared by [`CONSTANTS_COLLISION`]).
pub(super) const REGISTRATION_VANILLA: &[(&str, &[u8])] = &[(
    "common/scripted_variables/00_base_constants.txt",
    fixture!("registration-vanilla/common/scripted_variables/00_base_constants.txt"),
)];

/// `r1-target`'s trigger, effect, and constant registration cases, restated: same-file and
/// cross-file duplicates for each of the three rows, plus one technology file consuming a
/// locally-overridden constant, a vanilla one, and a cross-file-won one.
pub(super) const REGISTRATION: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("registration/descriptor.mod")),
    (
        "common/scripted_triggers/zz_dup_triggers_a.txt",
        TRIGGERS_A_BODY,
    ),
    (
        "common/scripted_triggers/zz_dup_triggers_b.txt",
        TRIGGERS_B_BODY,
    ),
    (
        "common/scripted_effects/zz_dup_effects.txt",
        fixture!("registration/common/scripted_effects/zz_dup_effects.txt"),
    ),
    (
        "common/scripted_variables/zz_dup_constants_a.txt",
        CONSTANTS_A_BODY,
    ),
    (
        "common/scripted_variables/zz_dup_constants_b.txt",
        CONSTANTS_B_BODY,
    ),
    ("common/technology/zz_consumer_tech.txt", CONSUMER_TECH_BODY),
];

/// `r4-reordered`'s method, applied to the trigger pair and the constant pair: their two file
/// names are exchanged with each other, byte for byte. `r4_the_winner_follows_position_and_
/// not_content` asserts the swap is genuine before trusting what moves.
pub(super) const REGISTRATION_FLIPPED: &[(&str, &[u8])] = &[
    (
        "descriptor.mod",
        fixture!("registration-flipped/descriptor.mod"),
    ),
    (
        "common/scripted_triggers/zz_dup_triggers_a.txt",
        TRIGGERS_A_FLIPPED_BODY,
    ),
    (
        "common/scripted_triggers/zz_dup_triggers_b.txt",
        TRIGGERS_B_FLIPPED_BODY,
    ),
    (
        "common/scripted_effects/zz_dup_effects.txt",
        fixture!("registration-flipped/common/scripted_effects/zz_dup_effects.txt"),
    ),
    (
        "common/scripted_variables/zz_dup_constants_a.txt",
        CONSTANTS_A_FLIPPED_BODY,
    ),
    (
        "common/scripted_variables/zz_dup_constants_b.txt",
        CONSTANTS_B_FLIPPED_BODY,
    ),
    (
        "common/technology/zz_consumer_tech.txt",
        fixture!("registration-flipped/common/technology/zz_consumer_tech.txt"),
    ),
];

pub(super) const TRIGGERS_A_BODY: &[u8] =
    fixture!("registration/common/scripted_triggers/zz_dup_triggers_a.txt");
pub(super) const TRIGGERS_B_BODY: &[u8] =
    fixture!("registration/common/scripted_triggers/zz_dup_triggers_b.txt");
pub(super) const TRIGGERS_A_FLIPPED_BODY: &[u8] =
    fixture!("registration-flipped/common/scripted_triggers/zz_dup_triggers_a.txt");
pub(super) const TRIGGERS_B_FLIPPED_BODY: &[u8] =
    fixture!("registration-flipped/common/scripted_triggers/zz_dup_triggers_b.txt");

pub(super) const CONSTANTS_A_BODY: &[u8] =
    fixture!("registration/common/scripted_variables/zz_dup_constants_a.txt");
pub(super) const CONSTANTS_B_BODY: &[u8] =
    fixture!("registration/common/scripted_variables/zz_dup_constants_b.txt");
pub(super) const CONSTANTS_A_FLIPPED_BODY: &[u8] =
    fixture!("registration-flipped/common/scripted_variables/zz_dup_constants_a.txt");
pub(super) const CONSTANTS_B_FLIPPED_BODY: &[u8] =
    fixture!("registration-flipped/common/scripted_variables/zz_dup_constants_b.txt");

pub(super) const CONSUMER_TECH_BODY: &[u8] =
    fixture!("registration/common/technology/zz_consumer_tech.txt");

/// `r5-risky-constants` and `r7-risky-consumed`: a forward reference and a two-symbol cycle,
/// each consumed from its own technology file so one broken reference's blast radius cannot
/// be confused with another's.
pub(super) const RISKY_CONSTANTS: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("risky-constants/descriptor.mod")),
    (
        "common/scripted_variables/zz_risky.txt",
        fixture!("risky-constants/common/scripted_variables/zz_risky.txt"),
    ),
    (
        "common/technology/zz_fwd_consumer.txt",
        fixture!("risky-constants/common/technology/zz_fwd_consumer.txt"),
    ),
    (
        "common/technology/zz_cycle_consumer.txt",
        fixture!("risky-constants/common/technology/zz_cycle_consumer.txt"),
    ),
    (
        "common/technology/zz_undeclared_consumer.txt",
        fixture!("risky-constants/common/technology/zz_undeclared_consumer.txt"),
    ),
];

/// The scripted-constants cross-source open cell: `@shared_symbol` redeclared from the
/// Target Mod, plus one consumer of the colliding symbol and one of an uncontested one.
pub(super) const CONSTANTS_COLLISION: &[(&str, &[u8])] = &[
    (
        "descriptor.mod",
        fixture!("constants-collision/descriptor.mod"),
    ),
    (
        "common/scripted_variables/zz_collision.txt",
        fixture!("constants-collision/common/scripted_variables/zz_collision.txt"),
    ),
    (
        "common/technology/zz_collision_consumer.txt",
        fixture!("constants-collision/common/technology/zz_collision_consumer.txt"),
    ),
];

/// The `$PARAM$` reference open cell: a trigger carrying a nested `$COUNT$` substitution, one
/// carrying a root-level `$MODE$` key, plus a parameter-free control trigger in the same file.
pub(super) const PARAMETERIZED: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("parameterized/descriptor.mod")),
    (
        "common/scripted_triggers/zz_param_trigger.txt",
        fixture!("parameterized/common/scripted_triggers/zz_param_trigger.txt"),
    ),
];

pub(super) fn registration_vanilla() -> SourceSnapshot {
    corpus(SourceKind::VanillaContent, REGISTRATION_VANILLA)
}

// --- Phase 4H: events, buildings, megastructures, and ship components ---

pub(super) const EVENTS_VANILLA: &[(&str, &[u8])] = &[(
    "events/00_phase4h_events.txt",
    fixture!("events-vanilla/events/00_phase4h_events.txt"),
)];

pub(super) const EVENTS: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("events/descriptor.mod")),
    (
        "events/zz_phase4h_events.txt",
        fixture!("events/events/zz_phase4h_events.txt"),
    ),
];

pub(super) const EVENTS_EARLY: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("events-early/descriptor.mod")),
    (
        "events/!!!_phase4h_events.txt",
        fixture!("events-early/events/!!!_phase4h_events.txt"),
    ),
];

pub(super) const BUILDINGS_VANILLA: &[(&str, &[u8])] = &[(
    "common/buildings/00_phase4h_buildings.txt",
    fixture!("buildings-vanilla/common/buildings/00_phase4h_buildings.txt"),
)];

pub(super) const BUILDINGS: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("buildings/descriptor.mod")),
    (
        "common/buildings/zz_phase4h_buildings.txt",
        fixture!("buildings/common/buildings/zz_phase4h_buildings.txt"),
    ),
];

pub(super) const COMPONENTS: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("components/descriptor.mod")),
    (
        "common/component_templates/zz_phase4h_components.txt",
        fixture!("components/common/component_templates/zz_phase4h_components.txt"),
    ),
];

pub(super) const COMPONENTS_REPEAT: &[(&str, &[u8])] = &[
    (
        "descriptor.mod",
        fixture!("components-repeat/descriptor.mod"),
    ),
    (
        "common/component_templates/zz_phase4h_components_repeat.txt",
        fixture!("components-repeat/common/component_templates/zz_phase4h_components_repeat.txt"),
    ),
];

pub(super) fn events_vanilla() -> SourceSnapshot {
    corpus(SourceKind::VanillaContent, EVENTS_VANILLA)
}

pub(super) fn buildings_vanilla() -> SourceSnapshot {
    corpus(SourceKind::VanillaContent, BUILDINGS_VANILLA)
}

// --- Phase 4I: sprite definitions ---

pub(super) const SPRITES_VANILLA: &[(&str, &[u8])] = &[(
    "interface/alerts.gfx",
    fixture!("sprites-vanilla/interface/alerts.gfx"),
)];

pub(super) const SPRITES: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("sprites/descriptor.mod")),
    (
        "interface/zz_phase4i_sprites_a.gfx",
        fixture!("sprites/interface/zz_phase4i_sprites_a.gfx"),
    ),
    (
        "interface/zz_phase4i_sprites_b.gfx",
        fixture!("sprites/interface/zz_phase4i_sprites_b.gfx"),
    ),
];

pub(super) const SPRITES_EARLY: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("sprites-early/descriptor.mod")),
    (
        "interface/00_phase4i_sprites.gfx",
        fixture!("sprites-early/interface/00_phase4i_sprites.gfx"),
    ),
];

pub(super) fn sprites_vanilla() -> SourceSnapshot {
    corpus(SourceKind::VanillaContent, SPRITES_VANILLA)
}

// --- Phase 4J: localization file stream ---

pub(super) const LOCALIZATION_VANILLA: &[(&str, &[u8])] = &[
    (
        "localisation/english/main_2_l_english.yml",
        fixture!("localization-vanilla/localisation/english/main_2_l_english.yml"),
    ),
    (
        "localisation/english/technology_l_english.yml",
        fixture!("localization-vanilla/localisation/english/technology_l_english.yml"),
    ),
];

pub(super) const LOCALIZATION_METHODS: &[(&str, &[u8])] = &[
    (
        "descriptor.mod",
        fixture!("localization-methods/descriptor.mod"),
    ),
    (
        "localisation/english/00_early_l_english.yml",
        fixture!("localization-methods/localisation/english/00_early_l_english.yml"),
    ),
    (
        "localisation/english/zz_plain_l_english.yml",
        fixture!("localization-methods/localisation/english/zz_plain_l_english.yml"),
    ),
    (
        "localisation/english/replace/00_replace_l_english.yml",
        fixture!("localization-methods/localisation/english/replace/00_replace_l_english.yml"),
    ),
];

pub(super) const LOCALIZATION_SAME_PATH: &[(&str, &[u8])] = &[
    (
        "descriptor.mod",
        fixture!("localization-samepath/descriptor.mod"),
    ),
    (
        "localisation/english/technology_l_english.yml",
        fixture!("localization-samepath/localisation/english/technology_l_english.yml"),
    ),
];

pub(super) fn localization_vanilla() -> SourceSnapshot {
    corpus(SourceKind::VanillaContent, LOCALIZATION_VANILLA)
}

// --- Phase 4G: inline scripts ---

/// The base-game side [`INLINE`] expands against: the inline script whose path the mod
/// occupies, and one technology naming no inline script at all.
pub(super) const INLINE_VANILLA: &[(&str, &[u8])] = &[
    (
        "common/inline_scripts/technologies/rare_weight_modifiers.txt",
        fixture!("inline-vanilla/common/inline_scripts/technologies/rare_weight_modifiers.txt"),
    ),
    (
        "common/technology/00_inline_baseline_tech.txt",
        fixture!("inline-vanilla/common/technology/00_inline_baseline_tech.txt"),
    ),
];

/// `r11-inline`: simple expansion, `$PARAM$` substitution, recursive nesting, and a mod file
/// at a base-game inline script's path, each written to expand to one comparable shape.
pub(super) const INLINE: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("inline/descriptor.mod")),
    (
        "common/inline_scripts/oracle/factor_block.txt",
        fixture!("inline/common/inline_scripts/oracle/factor_block.txt"),
    ),
    (
        "common/inline_scripts/oracle/inner_factor.txt",
        fixture!("inline/common/inline_scripts/oracle/inner_factor.txt"),
    ),
    (
        "common/inline_scripts/oracle/outer_factor.txt",
        fixture!("inline/common/inline_scripts/oracle/outer_factor.txt"),
    ),
    (
        "common/inline_scripts/oracle/param_factor.txt",
        fixture!("inline/common/inline_scripts/oracle/param_factor.txt"),
    ),
    (
        "common/inline_scripts/technologies/rare_weight_modifiers.txt",
        fixture!("inline/common/inline_scripts/technologies/rare_weight_modifiers.txt"),
    ),
    (
        "common/technology/zz_inline_nested.txt",
        fixture!("inline/common/technology/zz_inline_nested.txt"),
    ),
    (
        "common/technology/zz_inline_tech.txt",
        fixture!("inline/common/technology/zz_inline_tech.txt"),
    ),
];

/// `r12-inline-missing`: a reference to a path no file supplies, and a sibling definition
/// after it that must resolve normally.
pub(super) const INLINE_MISSING: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("inline-missing/descriptor.mod")),
    (
        "common/technology/zz_missing_inline.txt",
        fixture!("inline-missing/common/technology/zz_missing_inline.txt"),
    ),
];

pub(super) fn inline_vanilla() -> SourceSnapshot {
    corpus(SourceKind::VanillaContent, INLINE_VANILLA)
}

// --- Phase 4K: the golden-case fixture shapes ---
//
// Unlike every corpus above, these three restate a *golden case* rather than an oracle record.
// Their expectations live in [`super::golden`] for that reason: `oracle` is where a claim rests
// on a captured observation of the game, and none of these has one. What they can honestly
// carry today is stated case by case there.

/// The base side of [`MALFORMED`]: one key a faulted mod file never contests.
pub(super) const MALFORMED_VANILLA: &[(&str, &[u8])] = &[(
    "common/technology/00_malformed_baseline_tech.txt",
    fixture!("malformed-vanilla/common/technology/00_malformed_baseline_tech.txt"),
)];

/// Golden case 4's shape: one fault that costs a definition, one that costs only evidence
/// quality, and one wholly clean file in the same corpus.
pub(super) const MALFORMED: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("malformed/descriptor.mod")),
    (
        "common/technology/malformed_intact.txt",
        fixture!("malformed/common/technology/malformed_intact.txt"),
    ),
    (
        "common/technology/malformed_recovery.txt",
        MALFORMED_RECOVERY_BODY,
    ),
    (
        "common/technology/malformed_stray_brace.txt",
        MALFORMED_STRAY_BODY,
    ),
];

/// Named separately so the parser-seam expectations can read the same bytes the corpus does.
/// A second `include_bytes!` of the same path would compile just as well and would be a second
/// place to edit when the fixture changes.
pub(super) const MALFORMED_RECOVERY_BODY: &[u8] =
    fixture!("malformed/common/technology/malformed_recovery.txt");
pub(super) const MALFORMED_STRAY_BODY: &[u8] =
    fixture!("malformed/common/technology/malformed_stray_brace.txt");

/// The base side of [`ZERO_WEIGHT`]: one uncontested key with no `weight_modifier` at all.
pub(super) const ZERO_WEIGHT_VANILLA: &[(&str, &[u8])] = &[(
    "common/technology/00_zero_weight_baseline_tech.txt",
    fixture!("zero-weight-vanilla/common/technology/00_zero_weight_baseline_tech.txt"),
)];

/// Golden case 2's shape: a `factor = 0` modifier on a technology whose base weight is nonzero,
/// its matched control, and the prerequisite decoy `D-008` names.
pub(super) const ZERO_WEIGHT: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("zero-weight/descriptor.mod")),
    (
        "common/technology/zz_zero_weight_tech.txt",
        fixture!("zero-weight/common/technology/zz_zero_weight_tech.txt"),
    ),
];

/// The base side of [`ENIGMALITH`]: the constants its technologies read across sources.
pub(super) const ENIGMALITH_VANILLA: &[(&str, &[u8])] = &[(
    "common/scripted_variables/00_enigmalith_constants.txt",
    fixture!("enigmalith-vanilla/common/scripted_variables/00_enigmalith_constants.txt"),
)];

/// Golden case 3's shape: a zero base Draw Weight fed by a constant, two enclosing actions
/// granting that technology, and a megastructure entry the row cannot yet interpret.
pub(super) const ENIGMALITH: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("enigmalith/descriptor.mod")),
    (
        "common/megastructures/zz_enigmalith_megastructures.txt",
        ENIGMALITH_MEGASTRUCTURE_BODY,
    ),
    (
        "common/technology/zz_enigmalith_tech.txt",
        fixture!("enigmalith/common/technology/zz_enigmalith_tech.txt"),
    ),
    (
        "events/zz_enigmalith_events.txt",
        fixture!("enigmalith/events/zz_enigmalith_events.txt"),
    ),
];

/// Named separately because the megastructures row refuses before reading a file, so the only
/// way to show the corpus contains the entry at all is to parse these bytes directly.
pub(super) const ENIGMALITH_MEGASTRUCTURE_BODY: &[u8] =
    fixture!("enigmalith/common/megastructures/zz_enigmalith_megastructures.txt");

pub(super) fn malformed_vanilla() -> SourceSnapshot {
    corpus(SourceKind::VanillaContent, MALFORMED_VANILLA)
}

pub(super) fn zero_weight_vanilla() -> SourceSnapshot {
    corpus(SourceKind::VanillaContent, ZERO_WEIGHT_VANILLA)
}

pub(super) fn enigmalith_vanilla() -> SourceSnapshot {
    corpus(SourceKind::VanillaContent, ENIGMALITH_VANILLA)
}

const TECHNOLOGY_SCOPE: FileScope = FileScope {
    directory: "common/technology",
    extensions: &["txt"],
    recursive: false,
};

const EVENT_SCOPE: FileScope = FileScope {
    directory: "events",
    extensions: &["txt"],
    recursive: false,
};

/// A row with every cell settled, so a test can vary exactly one of them.
const fn settled(
    name: &'static str,
    scope: FileScope,
    duplicates: RepeatRule,
    fields: FieldRule,
    kinds: &'static [FactKind],
) -> RegistryPolicy {
    RegistryPolicy {
        name,
        key: CellStatus::Resolved(KeyRule {
            reader: DefinitionReader::TopLevelDefinitions,
            shadow: ShadowUnit::CommonFileSelection,
        }),
        stream: CellStatus::Resolved(StreamScope {
            family: ContentFamily::Script,
            scope,
        }),
        duplicates: CellStatus::Resolved(duplicates),
        cross_source: CellStatus::Resolved(CrossSourceRule::DecidedByStreamPosition),
        fields: CellStatus::Resolved(fields),
        ordering: CellStatus::Resolved(OrderingRule::SourceOrderPreserved),
        references: CellStatus::Resolved(NO_REFERENCES),
        provenance: CellStatus::Resolved(ProvenanceRule { kinds }),
    }
}

const WHOLE_OBJECT: FieldRule = FieldRule {
    replacement: Replacement::WholeObject,
    defaults: &[],
};

const CONTESTED_KINDS: &[FactKind] = &[
    FactKind::Contributed,
    FactKind::Duplicate,
    FactKind::Shadowed,
];

/// Later replaces earlier. To win, a mod's file must sort **after** the one it overrides —
/// the group technologies, buildings, megastructures, scripted triggers, and effects belong
/// to.
pub(super) const REPLACE_ON_REPEAT: RegistryPolicy = settled(
    "trial-replace-on-repeat",
    TECHNOLOGY_SCOPE,
    RepeatRule::ReplaceOnRepeat,
    WHOLE_OBJECT,
    CONTESTED_KINDS,
);

/// Later is rejected. To win, a mod's file must sort **before** — the group events and
/// scripted constants belong to.
pub(super) const REJECT_ON_REPEAT: RegistryPolicy = settled(
    "trial-reject-on-repeat",
    EVENT_SCOPE,
    RepeatRule::RejectOnRepeat,
    WHOLE_OBJECT,
    CONTESTED_KINDS,
);

/// The same scope as [`REPLACE_ON_REPEAT`] with the rule inverted.
///
/// Not a row anybody would declare: it exists so the r10 pair can be shown to *fail* when
/// the repeat rule is wrong, without editing the row the passing assertion uses.
pub(super) const REPLACE_SCOPE_REJECTING: RegistryPolicy = settled(
    "trial-replace-scope-rejecting",
    TECHNOLOGY_SCOPE,
    RepeatRule::RejectOnRepeat,
    WHOLE_OBJECT,
    CONTESTED_KINDS,
);

/// The same scope as [`REJECT_ON_REPEAT`] with the rule inverted, for the other half of the
/// same negative control.
pub(super) const EVENT_SCOPE_REPLACING: RegistryPolicy = settled(
    "trial-event-scope-replacing",
    EVENT_SCOPE,
    RepeatRule::ReplaceOnRepeat,
    WHOLE_OBJECT,
    CONTESTED_KINDS,
);

pub(super) const fn with_fields(
    name: &'static str,
    fields: FieldRule,
    kinds: &'static [FactKind],
) -> RegistryPolicy {
    settled(
        name,
        TECHNOLOGY_SCOPE,
        RepeatRule::ReplaceOnRepeat,
        fields,
        kinds,
    )
}

/// The same scope as [`REPLACE_ON_REPEAT`] with both reference kinds declared.
///
/// The undeclaring rows above refuse when a definition carries a reference, which is the right
/// answer for them and the wrong shape for a test that needs to see a reference *recorded*.
pub(super) const TECHNOLOGY_DETECTING_REFERENCES: RegistryPolicy = RegistryPolicy {
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
        ],
    }),
    ..settled(
        "trial-technology-detecting-references",
        TECHNOLOGY_SCOPE,
        RepeatRule::ReplaceOnRepeat,
        WHOLE_OBJECT,
        CONTESTED_KINDS,
    )
};

/// The technologies row's field rule inverted, for the control that separates whole-object
/// replacement from inheritance.
///
/// The two rules agree on every assertion golden case 5 makes *except* whether an omitted
/// field comes back, which is the whole reason the omitted-`potential` case is the design's
/// first mandatory oracle case. Declared on a copy so the control cannot be left switched on,
/// and declaring [`FactKind::Inherited`] so the control fails on the field rule rather than on
/// an undeclared kind — a refusal would be a pass for the wrong reason.
pub(super) const TECHNOLOGY_SCOPE_INHERITING: RegistryPolicy = with_fields(
    "trial-technology-scope-inheriting",
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

// --- Phase 4F engine-test rows: Parameter and ResolvedAgainstConstants ---

pub(super) const TRIGGERS_SCOPE: FileScope = FileScope {
    directory: "common/scripted_triggers",
    extensions: &["txt"],
    recursive: false,
};

/// A trigger row declaring every kind, `Parameter` left `Pending` — the open-cell shape:
/// resolves a parameter-free corpus, refuses only once a definition actually carries one.
pub(super) const TRIGGERS_DECLARING_PARAMETER: RegistryPolicy = RegistryPolicy {
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
            (
                ReferenceKind::Parameter,
                CellStatus::Pending {
                    reason: "no record measures $PARAM$ substitution in a trigger or effect \
                             body",
                    oracle_gap: "a capture exercising a parameterised scripted trigger/effect \
                                 call",
                },
            ),
        ],
    }),
    ..settled(
        "trial-triggers-declaring-parameter",
        TRIGGERS_SCOPE,
        RepeatRule::ReplaceOnRepeat,
        WHOLE_OBJECT,
        CONTESTED_KINDS,
    )
};

/// A trigger row declaring no reference kind at all — the control for the undeclared-kind
/// refusal: `Parameter` is neither detected nor pending, so encountering one refuses outright
/// rather than deferring.
pub(super) const TRIGGERS_NO_REFERENCES: RegistryPolicy = settled(
    "trial-triggers-no-references",
    TRIGGERS_SCOPE,
    RepeatRule::ReplaceOnRepeat,
    WHOLE_OBJECT,
    CONTESTED_KINDS,
);

/// A technology-shaped row declaring `Parameter` as `DetectedNotResolved`, for the nested-key
/// detection test: `$PARAM$` used as a nested field's own key, not its value.
pub(super) const TECHNOLOGY_DETECTING_PARAMETER: RegistryPolicy = RegistryPolicy {
    references: CellStatus::Resolved(ReferenceRule {
        kinds: &[(
            ReferenceKind::Parameter,
            CellStatus::Resolved(ReferenceHandling::DetectedNotResolved),
        )],
    }),
    ..settled(
        "trial-technology-detecting-parameter",
        TECHNOLOGY_SCOPE,
        RepeatRule::ReplaceOnRepeat,
        WHOLE_OBJECT,
        CONTESTED_KINDS,
    )
};

/// A technology-shaped row that resolves `@constant` references against the constants
/// environment, for the engine test proving `ConstantFact` — never `ReferenceFact` — is what
/// a `ResolvedAgainstConstants` kind produces.
pub(super) const TECHNOLOGY_RESOLVING_CONSTANTS: RegistryPolicy = RegistryPolicy {
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
    ..settled(
        "trial-technology-resolving-constants",
        TECHNOLOGY_SCOPE,
        RepeatRule::ReplaceOnRepeat,
        WHOLE_OBJECT,
        CONTESTED_KINDS,
    )
};

/// The scripted-triggers row's repeat rule inverted — the `r1`/`r4` direction negative
/// control: declared on a copy so the control cannot be left switched on.
pub(super) const TRIGGERS_SCOPE_REJECTING: RegistryPolicy = settled(
    "trial-triggers-scope-rejecting",
    TRIGGERS_SCOPE,
    RepeatRule::RejectOnRepeat,
    WHOLE_OBJECT,
    CONTESTED_KINDS,
);

/// The constants row itself, shaped exactly like the shipped `scripted-constants` row but
/// under a trial name — for engine tests that must not exercise the shipped row directly.
pub(super) const CONSTANTS_ROW: RegistryPolicy = RegistryPolicy {
    name: "trial-scripted-constants",
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
    fields: CellStatus::Resolved(WHOLE_OBJECT),
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

/// [`CONSTANTS_ROW`] with its repeat rule inverted — the other half of the `r1`/`r4`
/// direction negative control. `cross_source` is resolved rather than left `Pending`: the
/// `registration/` corpus this control runs over has no cross-source repeat, so the cell is
/// never consulted, and leaving it open would test nothing about this control's purpose.
pub(super) const CONSTANTS_ROW_REPLACING: RegistryPolicy = RegistryPolicy {
    duplicates: CellStatus::Resolved(RepeatRule::ReplaceOnRepeat),
    cross_source: CellStatus::Resolved(CrossSourceRule::DecidedByStreamPosition),
    ..CONSTANTS_ROW
};
