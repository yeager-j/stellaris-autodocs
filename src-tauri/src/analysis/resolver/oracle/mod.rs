//! Pinned oracle-record facts, encoded as machine-checked resolver expectations.
//!
//! # The pattern row tickets reuse
//!
//! An expectation is three things held together:
//!
//! 1. **A named record.** [`Expectation::run`] points at a directory under
//!    `docs/spikes/oracle-records/`, which holds the manifest, the extracted facts, and the
//!    normalized `error.log` a licensed local run produced.
//! 2. **A restated fixture.** The records observe vanilla content, and their observable is
//!    the game's own log. Neither can be shipped: vanilla bytes are licensed, and the log
//!    needs the game. `fixtures/resolver/` restates the *shape* each record established in
//!    original files, so the rule is checked in ordinary CI on a machine with no Stellaris
//!    installed. `fixtures/resolver/README.md` records why restating beats reusing the
//!    frozen oracle fixtures.
//! 3. **A drift gate.** Every consumed record is compared against the profile's pinned
//!    Stellaris build and against the artifacts on disk beside it, so a re-capture under a
//!    new game version blocks the suite instead of silently changing what "the oracle says".
//!
//! The gap between (1) and (2) is real and worth naming: this suite proves the resolver
//! implements the rule the record established, not that the record is still true of the
//! installed game. Only re-running the oracle harness proves that, and the drift gate is
//! what forces the question to be asked when the ground moves.
//!
//! # Negative controls
//!
//! Four run in ordinary CI as tests of their own:
//! [`the_pair_fails_when_either_repeat_rule_is_inverted`],
//! [`merge_by_key_would_keep_the_shadowed_file_s_other_keys`],
//! [`golden_case_5_fails_when_either_the_repeat_rule_or_the_field_rule_is_inverted`], and
//! `record::tests`.
//!
//! Those show the *assertions* discriminate. What they cannot show is that the assertions
//! catch a wrong **implementation**, because each varies a policy rather than the engine. So
//! each production rule was also broken by hand once, and the failure recorded here:
//!
//! - **A layer model.** Sorting the script stream by source before path — the single most
//!   likely wrong implementation, and the one this whole module is shaped to prevent:
//!
//! ```text
//! r10_an_early_sorting_mod_file_wins_the_rejecting_registry_and_loses_the_replacing_one
//! r10_provenance_records_a_position_rather_than_a_layer
//! stream::tests::script_interleaves_both_sources_by_path_with_no_rank
//! ```
//!
//! - **Merge-by-key.** Letting files removed by common file selection contribute to the
//!   stream anyway. The failure is exactly the two keys the winning file never mentioned,
//!   which is the only observable separating the two rules:
//!
//! ```text
//! left:  ["tech_contested", "tech_first_lost", "tech_second_lost", "tech_sentinel", "tech_untouched"]
//! right: ["tech_contested", "tech_sentinel", "tech_untouched"]
//! ```
//!
//! - **One repeat rule for every registry.** Forcing the engine to `RejectOnRepeat`
//!   regardless of the row failed nine tests, including both halves of the r10 pair.
//! - **A re-captured game build.** Pinning `v4.5.0` in `profile::SUPPORTED_STELLARIS_BUILD`
//!   failed the drift gate once per consumed record, with the re-capture instruction
//!   attached.
//! - **Provenance that names the wrong thing.** Recording the excluded file's own path as
//!   the `replace_path` declaration failed the r3 expectation on both excluded files.
//! - **The resolved-surface scan.** A line naming the file-level parse model in
//!   `resolved.rs`'s shipped portion failed `resolved_output_names_no_parse` at that line.
//! - **The version coupling.** Bumping `RESOLUTION_PROFILE_VERSION` failed
//!   `analysis::version::tests::pinned_current_digest`, which is the re-pin protocol working
//!   rather than a nuisance.
//!
//! Phase 4E added five more, each broken in the *shipped* row or engine rather than on a copy:
//!
//! - **A first-wins technologies row.** Forcing `RepeatRule::RejectOnRepeat` in the declared
//!   row failed both halves of the golden slice, and nothing else:
//!
//! ```text
//! r1_a_redefinition_that_omits_potential_leaves_the_field_absent
//! r4_the_winner_follows_position_and_not_content
//! ```
//!
//! - **An inheriting technologies row.** Forcing `Replacement::InheritAbsentFields` failed
//!   exactly one test — `r1_…_leaves_the_field_absent` — because every other assertion in the
//!   golden slice passes under both field rules. That is the omitted-`potential` case earning
//!   its place as the design's first mandatory oracle case, measured rather than asserted.
//! - **Reference detection skipped.** Returning no references from `registry::detect_references`
//!   failed the two deferred pins and the undeclared-kind refusal:
//!
//! ```text
//! profile::tests::a_scripted_constant_reference_is_detected_and_left_unresolved
//! profile::tests::an_inline_script_reference_is_detected_and_left_unexpanded
//! registry::tests::a_reference_kind_the_row_did_not_declare_refuses
//! ```
//!
//! - **A reader that took every top-level field.** Dropping the constant-declaration skip from
//!   `registry::top_level_definitions` failed
//!   `a_file_local_constant_declaration_is_not_read_as_a_definition`, and filtering *before*
//!   assigning ordinals — renumbering rather than leaving a gap — failed
//!   `skipping_a_declaration_leaves_a_gap_rather_than_renumbering`. Two faults, two tests, one
//!   for each.
//! - **A walk with a blind spot.** Dropping the tagged container from
//!   `registry::Scan::walk_value` failed `a_reference_inside_a_tagged_container_is_found`,
//!   which is what says that test exercises the tagged path rather than passing through some
//!   other one.
//! - **A flip that was not a flip.** Renaming the `redefinition-flipped` corpus's file to sort
//!   *after* vanilla failed `r4_the_winner_follows_position_and_not_content` alone, which is
//!   what makes that test a position experiment rather than a restatement of `r1`.
//! - **A re-captured game build, over the newly consumed records.** Pinning `v4.5.0` named
//!   `r0-baseline`, `r1-target`, and `r4-reordered` alongside the three the core already
//!   consumed, so the three records this row rests on are under the same gate.
//!
//! Phase 4F (STE-27) added ten more, each broken in the *shipped* row, the shared engine, or
//! a committed fixture rather than on a copy — each performed by hand once against the
//! shipped code, the named test observed failing, then restored:
//!
//! - **The constants direction inverted.** Forcing `RepeatRule::ReplaceOnRepeat` on the
//!   shipped `scripted-constants` row failed its own direction tests and nothing about
//!   triggers:
//!
//! ```text
//! r1_scripted_constants_reject_on_repeat_first_wins
//! r4_the_trigger_and_constant_winners_follow_position_and_not_content
//! ```
//!
//! - **The triggers direction inverted**, the converse: forcing `RepeatRule::RejectOnRepeat`
//!   on the shipped `scripted-triggers` row failed exactly the trigger/effect half instead —
//!   `r1_scripted_triggers_and_effects_replace_on_repeat_last_wins` and the same combined
//!   `r4_…` test — and left every constants test green.
//! - **An un-swapped flip corpus.** Copying `registration/`'s trigger and constant files
//!   over `registration-flipped/`'s (undoing the name exchange, content otherwise identical)
//!   failed `r4_the_trigger_and_constant_winners_follow_position_and_not_content` alone,
//!   which is what makes that test a position experiment on this row pair rather than a
//!   restatement of `registration/`'s own same- and cross-file results.
//! - **Two-pass/fixpoint chain evaluation.** Letting a second pass re-derive a forward
//!   reference once every symbol has been seen once — rather than looking each declaration
//!   up only against symbols already settled earlier in the one stream-ordered pass — failed
//!   the forward-reference and cycle tests, plus the consuming oracle case built on them:
//!
//! ```text
//! constants::tests::a_forward_reference_does_not_resolve
//! constants::tests::a_cycle_does_not_resolve_and_the_pass_terminates
//! r5_and_r7_a_forward_reference_and_a_cycle_never_present_a_fabricated_value
//! ```
//!
//! - **Local-override fall-through to the global.** Skipping the local declaration in
//!   `Environment::lookup` and falling straight to the global binding turned 99 back into 11
//!   in `r1_the_technologies_row_consumes_registered_constants`, alongside the two
//!   `constants::tests` cases exercising the same path directly.
//! - **A silent lookup miss.** Recording a `ConstantFact` only when the outcome resolves —
//!   dropping the fact entirely on any `Unresolved` outcome instead of recording it — failed
//!   `r5_and_r7_a_forward_reference_and_a_cycle_never_present_a_fabricated_value` and the
//!   rewritten profile pin,
//!   `profile::tests::a_scripted_constant_reference_is_resolved_against_the_constants_environment`,
//!   which is exactly the silent incompleteness a typed outcome exists to prevent.
//! - **`f64` instead of `ExactValue`.** `0.1_f64 + 0.2_f64 == 0.3_f64` is `false` in ordinary
//!   binary floating point (`0.1 + 0.2` renders as `0.30000000000000004`); `constants::tests::
//!   decimal_addition_is_exact_through_exact_value` passes only because it goes through
//!   `ExactValue::add`, never `f64` arithmetic — the control here *is* that well-known
//!   finding, not a line to comment out.
//! - **Each open cell closed by hand.** Resolving the scripted-constants row's cross-source
//!   cell failed exactly `the_scripted_constants_cross_source_cell_refuses_only_on_a_
//!   collision`; resolving `PARAMETER_OPEN` to `DetectedNotResolved` failed exactly
//!   `the_parameter_open_cell_refuses_only_when_a_definition_carries_one`. Neither touched
//!   the other cell's test, which is what says the two open cells are independent claims.
//! - **Parameter detection removed from `Scan::walk_scalar`.** Failed the parameter refusal
//!   and the undeclared-kind control, and the oracle open-cell test built on the same path:
//!
//! ```text
//! registry::tests::an_open_reference_kind_cell_refuses_only_when_a_definition_carries_it
//! registry::tests::an_undeclared_parameter_kind_refuses
//! the_parameter_open_cell_refuses_only_when_a_definition_carries_one
//! ```
//!
//!   `registry::tests::a_parameter_used_as_a_nested_field_key_is_detected` stayed green,
//!   which is what separates the nested-key check from the scalar-value one: they are two
//!   mechanisms, not one, and this is the control that says so.
//! - **The drift gate, over the two newly consumed records.** `r5-risky-constants` and
//!   `r7-risky-consumed` are named in `EXPECTATIONS` alongside the records earlier phases
//!   consumed, so `every_consumed_record_matches_the_profile_s_pinned_build` covers them
//!   mechanically — a re-capture under a new Stellaris build would fail that one gate for
//!   both, the same as it already does for `r0`, `r1`, `r3`, `r4`, `r6`, and `r10`.
//!
//! A Codex review of the shipped Phase 4F code (PR #15) found three more faults — none a
//! cell or a row's declared policy, so no `RESOLUTION_PROFILE_VERSION` bump accompanies
//! them; each just makes the engine honour what version 3 already declares. Each was broken
//! by hand once against the fixed shipped code and the named test observed failing, then
//! restored:
//!
//! - **The root-key Parameter walk removed** (`registry::detect_references`). A `$PARAM$`
//!   key used as a definition's own top-level field — not nested inside a container — escaped
//!   detection entirely, because `EffectiveField` flattens a key to its text and erases the
//!   `ScalarKind` that names it a parameter. Removing the walk over the winning body's own
//!   container failed exactly the two root-level tests and nothing else:
//!
//! ```text
//! registry::tests::a_root_level_parameter_key_refuses_under_an_open_cell
//! registry::tests::a_root_level_parameter_key_is_detected_when_declared
//! ```
//!
//! - **The alias-propagation pass removed** (`constants::build_environment`). Without it, a
//!   symbol whose value was copied from another symbol during chain evaluation (`@alias =
//!   @base`) kept that copied value even after the second pass marked `@base` itself
//!   `CrossSourcePending` — a consumer would see a resolved number derived from a binding the
//!   row explicitly refuses to stand behind. Failed the alias unit test and its
//!   consuming-side twin:
//!
//! ```text
//! constants::tests::cross_source_invalidation_propagates_through_an_alias
//! registry::tests::a_consumer_reading_an_alias_of_a_contested_symbol_is_pending
//! ```
//!
//! - **Global seeding restored for local bodies** (`constants::Environment::lookup`). Passing
//!   the global environment back into a local declaration's own evaluation — the shape before
//!   this fix — turns `@cost = @cost` into a silent read of the very global `@cost` the local
//!   declaration exists to shadow, fabricating a value for a self-cycle. Only a literal local
//!   override is measured (`r1`); failed both reference-bodied local tests and left the
//!   pre-existing literal-99 tests unaffected, confirming the control isolates the reference
//!   shape rather than breaking local overrides generally:
//!
//! ```text
//! constants::tests::a_self_referencing_local_declaration_never_falls_through_to_the_global
//! constants::tests::a_local_declaration_referencing_a_different_symbol_is_equally_unmeasured
//! ```
//!
//! Phase 4G (STE-28) added four, each broken by hand once in the *shipped* expander and the
//! named tests observed failing, then restored:
//!
//! - **Single-level expansion**, the ticket's named control: deleting the recursion into
//!   spliced content in `inline_scripts::Expansion::include`. The result is stronger than a
//!   wrong answer — a one-level expander leaves the outer fragment's own `inline_script` field
//!   standing in the effective value, which trips `registry::Scan::record`'s
//!   `ExpandedFromInlineScripts` defect assertion for every definition resolved from that
//!   corpus, so it cannot silently publish a technology missing its weight logic. To show the
//!   nesting *assertion* discriminates rather than just the guard, the break was repeated with
//!   the `unreachable!` neutralized to a no-op; exactly four tests then failed on their own
//!   assertions, and `r11_nested_inclusion_expands_recursively` is the only oracle expectation
//!   among them — the other five `r11` subjects stayed green:
//!
//! ```text
//! r11_nested_inclusion_expands_recursively
//! inline_scripts::tests::a_nested_inclusion_expands_recursively
//! inline_scripts::tests::a_cyclic_inclusion_terminates_with_a_typed_outcome
//! inline_scripts::tests::a_failing_nested_site_does_not_fail_its_parent_site
//! ```
//!
//! - **Substitution removed.** Making `inline_scripts::substitute_scalar` a no-op failed
//!   exactly the three parameter tests and nothing else — the unbound-parameter check catches
//!   the surviving `$F$` and omits the inclusion, so the failure is a missing modifier rather
//!   than a `Parameter` scalar leaking into an effective field:
//!
//! ```text
//! r11_parameters_substitute_into_the_expanded_content
//! inline_scripts::tests::a_bound_parameter_substitutes_in_a_value_position
//! inline_scripts::tests::a_bound_parameter_substitutes_in_a_nested_key_position
//! ```
//!
//! - **The typed fact dropped on an unknown path.** Returning no items *without* recording
//!   the fact — the shape where an inclusion silently vanishes, which is precisely the hazard
//!   `r12` names — failed the r12 expectation and the three unit tests that read a failing
//!   site's fact:
//!
//! ```text
//! r12_an_unresolved_inline_reference_is_a_typed_fact_and_the_definition_survives
//! inline_scripts::tests::an_unknown_path_omits_the_inclusion_and_records_the_reference
//! inline_scripts::tests::a_failing_site_leaves_its_siblings_expanded
//! inline_scripts::tests::a_failing_nested_site_does_not_fail_its_parent_site
//! ```
//!
//! - **Provenance taken from the call site instead of the script file.** Recording the
//!   consuming definition's position as `InlineOutcome::Expanded`'s `script` failed the three
//!   tests that read *where the expanded content came from*, which is what separates "an
//!   inclusion expanded" from "this content came from the mod's file at the vanilla path":
//!
//! ```text
//! r11_a_simple_inline_script_expands_into_the_consuming_definition
//! r11_a_mod_file_at_a_vanilla_script_s_path_overrides_its_content
//! profile::tests::an_inline_script_reference_is_expanded_into_the_consuming_definition
//! ```
//!
//!   The override result itself has no *new* code to break: a mod file at a vanilla inline
//!   script's path wins through `selection`'s exact-path rule, before any stream exists, and
//!   that rule already carries its own controls (`r6_an_exact_path_collision_removes_the_
//!   whole_losing_file`, `merge_by_key_would_keep_the_shadowed_file_s_other_keys`). What the
//!   override expectation adds is that the *library* reads the selected file set rather than a
//!   snapshot, and the control above is what shows that test reads the source it names.
//!
//! - **The drift gate, over the two newly consumed records.** `r11-inline` and
//!   `r12-inline-missing` are named in `EXPECTATIONS`, so
//!   `every_consumed_record_matches_the_profile_s_pinned_build` covers them mechanically,
//!   exactly as it already does for the records earlier phases consumed.
//!
//! A Codex review of the shipped Phase 4G code (PR #16) found one more fault — not a cell and
//! not a row's declared policy, so no `RESOLUTION_PROFILE_VERSION` bump accompanies it; it
//! makes the engine honour what version 4 already declares on input version 4 never
//! contemplated. Broken by hand once against the fixed shipped code and the named test
//! observed failing, then restored:
//!
//! - **The expansion site budget removed** (`inline_scripts::Expansion::include`). Cycle
//!   detection guards the ancestor chain, which is not the same as bounding the work: a
//!   fragment that includes the next one *twice*, nested `k` deep, is entirely acyclic and
//!   describes 2^k sites, so thirteen tiny files ask for 8191 expansions and a few more never
//!   finish. Mod content is untrusted input. Deleting the budget check failed exactly the
//!   pathological case and left the legitimate-nesting control green, which is what says the
//!   bound discriminates a pathological corpus rather than limiting real content:
//!
//! ```text
//! inline_scripts::tests::a_doubling_chain_stops_at_the_expansion_budget
//! ```
//!
//!   `inline_scripts::tests::legitimate_nesting_stays_far_below_the_expansion_budget` stayed
//!   green under the same break, and `r11_nested_inclusion_expands_recursively` with it.
//!
//! Phase 4H (STE-29) added four hand-broken controls against the shipped paths, each restored
//! before the next control:
//!
//! - Forcing the declared events row to `ReplaceOnRepeat` failed
//!   `r9_events_keep_the_first_same_source_registration_and_record_the_loser`: the second
//!   same-file event became ordinal 2 instead of the retained first event at ordinal 1.
//! - Reading an inner-key row by its top-level block label failed
//!   `an_inner_field_reader_uses_the_direct_identifier_not_the_shared_block_name`: two
//!   `event` blocks produced `event`, `event` rather than `story.1`, `story.2`.
//! - Moving the duplicate-cell consult back before the stream failed
//!   `an_open_duplicate_cell_is_lazy_but_refuses_at_the_first_repeat`: its clean corpus could
//!   no longer resolve before a duplicate decision was needed.
//! - Changing buildings to `InheritAbsentFields` failed `r8_buildings_replace_the_whole_object`
//!   at the provenance contract: the engine produced an undeclared `Inherited` fact, before it
//!   could silently publish the inherited `building_sets` field.
//!
//! Phase 4I (STE-30) carries the ticket's reference-resolution negative control in ordinary
//! CI. `resolving_sheet_edges_against_the_shadowed_definition_breaks_r17` first proves the
//! late corpus's Vanilla dependents resolve the Target Mod texture, substitutes the
//! byte-identical Vanilla sheet that r17 displaced, reruns the shipped reference post-pass,
//! and proves the same dependent assertion goes false. That isolates winner lookup from
//! registration direction: a row can select the right winner and still attribute every
//! dependent to the wrong source if its references consult the loser.

mod record;

use super::registry::{CellStatus, FieldRule, PolicyCell, Refusal, RepeatRule, Replacement};
use super::resolved::{
    ConstantOutcome, FactKind, FactSite, InlineOutcome, Removal, ResolvedDefinition,
    ResolvedRegistry, SpriteTextureOutcome, UnresolvedConstant, UnresolvedInline,
};
use super::trial::{
    self, BUILDINGS, COMPONENTS, COMPONENTS_REPEAT, CONSTANTS_A_BODY, CONSTANTS_A_FLIPPED_BODY,
    CONSTANTS_B_BODY, CONSTANTS_B_FLIPPED_BODY, CONSTANTS_COLLISION, EARLY_MOD, EVENTS,
    EVENTS_EARLY, INLINE, INLINE_MISSING, NO_REDEFINITION, PARAMETERIZED, PATH_COLLISION,
    REDEFINITION, REDEFINITION_BODY, REDEFINITION_FLIPPED, REDEFINITION_FLIPPED_BODY, REGISTRATION,
    REGISTRATION_FLIPPED, REPLACE_PATH, RISKY_CONSTANTS, SPRITES, SPRITES_EARLY, TRIGGERS_A_BODY,
    TRIGGERS_A_FLIPPED_BODY, TRIGGERS_B_BODY, TRIGGERS_B_FLIPPED_BODY, buildings_vanilla, corpus,
    events_vanilla, inline_vanilla, redefinition_vanilla, registration_vanilla, sprites_vanilla,
    vanilla,
};
use super::{Resolution, profile, resolve};
use crate::analysis::parser::Value;
use crate::canonical::path::LogicalPath;
use crate::source::fixture::FixtureCorpus;
use crate::source::{SourceKind, SourceSnapshot};

/// One captured run this suite holds the resolver to.
struct Expectation {
    run: &'static str,
    /// What the record established, in the words the evaluation uses for it.
    rule: &'static str,
}

/// The records consumed so far: the Phase 4D core's three, then each declared row's evidence.
///
/// Each row ticket appends its own. A record listed here is under the drift gate whether or
/// not an expectation below reads it in detail — `r0-baseline` is the clearest case, since its
/// value is the measured *absence* the r1 result is a delta against.
const EXPECTATIONS: &[Expectation] = &[
    Expectation {
        run: "r17-sprites",
        rule: "sprite names replace on repeat within and across files; a late-sorting Target \
               Mod sheet wins, and every Vanilla dependent resolves its texture through that \
               winning definition",
    },
    Expectation {
        run: "r18-sprites-early",
        rule: "an early-sorting Target Mod sheet registers before Vanilla and is then \
               replaced, proving sprites use global path order rather than source layers",
    },
    Expectation {
        run: "r8-registries",
        rule: "building and megastructure duplicate diagnostics use the new registration, while \
               event and component diagnostics identify collisions without naming a component winner; \
               the building comparison's omitted building_sets is whole-object replacement",
    },
    Expectation {
        run: "r9-events-runtime",
        rule: "the first registration of an event id actually evaluates at runtime; a later \
               same-file event with that id is rejected rather than replacing it",
    },
    Expectation {
        run: "r3-replace-path",
        rule: "replace_path excludes every other source's files in the directory; the \
               declaring mod's own files there still load",
    },
    Expectation {
        run: "r6-pathcollision",
        rule: "an exact path collision replaces the whole file; the losing file contributes \
               nothing, including keys the winner never mentions",
    },
    Expectation {
        run: "r10-loadorder",
        rule: "there is no layer precedence — one global path order, and each registry \
               either replaces or rejects a repeat",
    },
    Expectation {
        run: "r0-baseline",
        rule: "without the Target Mod the subject technology is absent from the draw pool, \
               which is what makes the r1 result a delta rather than a reading",
    },
    Expectation {
        run: "r1-target",
        rule: "technology redefinition is whole-object replacement: the subject omitting \
               `potential` was drawn, the matched control retaining it stayed absent, so an \
               omitted field is genuinely absent rather than inherited",
    },
    Expectation {
        run: "r4-reordered",
        rule: "byte-identical content under swapped filenames moves the winner, and the \
               technology result — whose file was not renamed — is unchanged, so r1 was not \
               a naming artifact",
    },
    Expectation {
        run: "r5-risky-constants",
        rule: "a forward reference and a two-symbol cycle are declared but never consumed, \
               and the game produces no diagnostic at all — an unconsumed broken constant is \
               invisible at load time, which is why detection cannot wait for the game's log",
    },
    Expectation {
        run: "r7-risky-consumed",
        rule: "consuming a forward reference corrupts the file with a diagnostic that never \
               names the constant (`unknown command 'tier' for MTTH/script value`), so \
               detection of an unresolved scripted constant is resolver-owned rather than \
               read off the error log",
    },
    Expectation {
        run: "r11-inline",
        rule: "inline scripts expand textually into the consuming definition before it \
               registers: a simple inclusion expands, `$PARAM$` substitutes, an inclusion \
               nests and must be expanded recursively, and a mod file at a vanilla script's \
               path replaces its content — all six subjects reached the draw pool and the \
               no-modifier control did not",
    },
    Expectation {
        run: "r12-inline-missing",
        rule: "an inline reference that does not resolve is diagnosed with the consuming file \
               and line, and the technology still registers with the inclusion silently \
               omitted — quieter than the file-corrupting cascade a broken scripted constant \
               produces, so an unexpanded inclusion must be a resolver-owned fact",
    },
];

fn against(
    files: &[(&str, &[u8])],
) -> (crate::source::SourceSnapshot, crate::source::SourceSnapshot) {
    (vanilla(), corpus(SourceKind::TargetMod, files))
}

fn row(resolution: &Resolution<'_>, policy: &super::registry::RegistryPolicy) -> ResolvedRegistry {
    resolution
        .resolve_row(policy)
        .unwrap_or_else(|refusal| panic!("a fully settled trial row resolves: {refusal}"))
}

/// The golden-case-5 corpus pair: the stand-in base game, and one Target Mod over it.
fn redefining(files: &[(&str, &[u8])]) -> (SourceSnapshot, SourceSnapshot) {
    (redefinition_vanilla(), corpus(SourceKind::TargetMod, files))
}

/// The declared technologies row, asked for the way a consumer asks — by name, through the
/// public seam. Golden case 5 is a claim about that surface, so `resolve_row` would prove a
/// weaker thing: that the policy works when handed directly to the engine, rather than that
/// the profile declares it and the registry answers to its name.
fn technologies(resolution: &Resolution<'_>) -> ResolvedRegistry {
    resolution
        .registry("technologies")
        .unwrap_or_else(|refusal| panic!("the declared technologies row resolves: {refusal}"))
}

/// A declared row resolved by name, panicking with the refusal on the caller's behalf. Named
/// separately from [`technologies`] because Phase 4F's expectations ask three different rows
/// by name, not one.
fn named(resolution: &Resolution<'_>, registry: &str) -> ResolvedRegistry {
    resolution
        .registry(registry)
        .unwrap_or_else(|refusal| panic!("the declared {registry} row resolves: {refusal}"))
}

/// `registration-vanilla/` paired with `registration/`: `r1-target`'s trigger, effect, and
/// constant registration cases, restated.
fn registration() -> (SourceSnapshot, SourceSnapshot) {
    (
        registration_vanilla(),
        corpus(SourceKind::TargetMod, REGISTRATION),
    )
}

/// `r4-reordered`'s method, applied to the trigger and constant pairs.
fn registration_flipped() -> (SourceSnapshot, SourceSnapshot) {
    (
        registration_vanilla(),
        corpus(SourceKind::TargetMod, REGISTRATION_FLIPPED),
    )
}

/// `r5-risky-constants` and `r7-risky-consumed`, restated.
fn risky_constants() -> (SourceSnapshot, SourceSnapshot) {
    (
        registration_vanilla(),
        corpus(SourceKind::TargetMod, RISKY_CONSTANTS),
    )
}

/// The scripted-constants cross-source open cell.
fn constants_collision() -> (SourceSnapshot, SourceSnapshot) {
    (
        registration_vanilla(),
        corpus(SourceKind::TargetMod, CONSTANTS_COLLISION),
    )
}

/// The `$PARAM$` reference open cell.
fn parameterized() -> (SourceSnapshot, SourceSnapshot) {
    (vanilla(), corpus(SourceKind::TargetMod, PARAMETERIZED))
}

fn megastructure_target() -> SourceSnapshot {
    FixtureCorpus::new(SourceKind::TargetMod)
        .with_file("descriptor.mod", b"name=\"phase4h-megastructure\"")
        .with_file(
            "common/megastructures/zz_phase4h_megastructure.txt",
            b"mega_phase4h = { icon = phase4h_icon }",
        )
        .build()
        .expect("a well-formed megastructure fixture corpus")
}

/// Effective field names in order. The full field-by-field comparison, for the fields whose
/// values are scalars, is [`scalar_fields`]; this is the shape check that comes first because
/// a missing field and a wrong value are different failures.
fn field_names(definition: &ResolvedDefinition) -> Vec<&str> {
    definition
        .fields
        .iter()
        .map(|field| field.field.as_str())
        .collect()
}

fn scalar_fields(definition: &ResolvedDefinition) -> Vec<(&str, String)> {
    definition
        .fields
        .iter()
        .filter_map(|field| match &field.value {
            Value::Scalar(scalar) => Some((field.field.as_str(), scalar.text().into_owned())),
            Value::Container(_) | Value::Tagged { .. } => None,
        })
        .collect()
}

/// The gate that makes every expectation below blocking.
///
/// Run as its own test so a drifted record fails once with a clear instruction, rather than
/// failing every expectation with a mismatched value and leaving the cause to be inferred.
#[test]
fn every_consumed_record_matches_the_profile_s_pinned_build() {
    let mut reasons = Vec::new();
    for expectation in EXPECTATIONS {
        let manifest = record::read(expectation.run);
        assert_eq!(
            manifest.run, expectation.run,
            "the record at {} names a different run",
            expectation.run
        );
        reasons.extend(record::drift(&manifest, profile::SUPPORTED_STELLARIS_BUILD));
    }
    assert!(
        reasons.is_empty(),
        "{}\n\n{}",
        reasons.join("\n"),
        record::INSTRUCTION
    );
}

#[test]
fn every_expectation_names_the_rule_its_record_established() {
    // Cheap, and it catches the failure mode this pattern is most prone to: an expectation
    // copied for a new row, left pointing at the previous row's record.
    for expectation in EXPECTATIONS {
        assert!(!expectation.rule.is_empty(), "{}", expectation.run);
    }
    let mut runs: Vec<&str> = EXPECTATIONS.iter().map(|e| e.run).collect();
    runs.sort_unstable();
    let count = runs.len();
    runs.dedup();
    assert_eq!(runs.len(), count, "one record consumed twice");
}

/// `r6-pathcollision`. The mod's file lands on a vanilla file's exact logical path and
/// defines a key that file never mentioned.
#[test]
fn r6_an_exact_path_collision_removes_the_whole_losing_file() {
    let (vanilla, target) = against(PATH_COLLISION);
    let resolution = resolve(&vanilla, &target);
    let registry = row(&resolution, &trial::REPLACE_ON_REPEAT);

    assert_eq!(
        registry.keys(),
        ["tech_contested", "tech_sentinel", "tech_untouched"],
        "the losing file's keys survived, so this is a merge and not a replacement"
    );

    // The scoping control, and the reason the corpus has two technology files:
    // `00_baseline_tech.txt` collided with nothing, so its keys must be untouched. Without
    // it, "resolution broke" would read exactly like "this file was displaced" — the same
    // separation r14's `tech_bio_reactor` provided in the localization case.
    assert!(registry.get("tech_untouched").is_some());

    let shadowed: Vec<&str> = registry
        .removed_files
        .iter()
        .filter(|fact| fact.kind == FactKind::Shadowed)
        .filter_map(|fact| fact.site.logical())
        .map(|logical| logical.as_str())
        .collect();
    assert_eq!(shadowed, ["common/technology/00_collided_tech.txt"]);
    assert!(
        registry.removed_files[0].field.is_none(),
        "a shadowed file is a fact about the file, not about one field of it"
    );
    let FactSite::RemovedBySelection {
        source, removal, ..
    } = &registry.removed_files[0].site
    else {
        panic!("a file removed by selection never entered a stream, so it has no position");
    };
    assert_eq!(*source, SourceKind::VanillaContent);
    assert_eq!(
        *removal,
        Removal::ShadowedByPathCollision {
            winner: SourceKind::TargetMod
        },
        "a collision and a directory exclusion both remove a file, and a reader that could \
         not tell them apart would not know whether the mod shipped a replacement"
    );
}

/// The negative control for the r6 assertion above.
///
/// Stated as "the keys the winner never mentions come back", because that is the *only*
/// observable that separates whole-file replacement from merge-by-key: every other
/// assertion in the r6 test passes under both rules.
#[test]
fn merge_by_key_would_keep_the_shadowed_file_s_other_keys() {
    let (vanilla, target) = against(PATH_COLLISION);
    let resolution = resolve(&vanilla, &target);
    let registry = row(&resolution, &trial::REPLACE_ON_REPEAT);
    let merged: Vec<&str> = registry
        .keys()
        .into_iter()
        .filter(|key| key.starts_with("tech_first") || key.starts_with("tech_second"))
        .collect();
    assert!(
        merged.is_empty(),
        "a merge-by-key resolver would produce {merged:?}, and this test would then be the \
         one that noticed"
    );
    // And the same corpus read without the collision does hold those keys, so their absence
    // above is the collision's doing rather than a fixture that never had them.
    let other_mod = corpus(SourceKind::TargetMod, EARLY_MOD);
    let untouched = resolve(&vanilla, &other_mod);
    let baseline = row(&untouched, &trial::REPLACE_ON_REPEAT);
    assert!(baseline.get("tech_first_lost").is_some());
    assert!(baseline.get("tech_second_lost").is_some());
}

/// `r3-replace-path`. Both halves in one test: either alone is satisfied by a wrong rule.
#[test]
fn r3_replace_path_excludes_other_sources_and_keeps_the_declarer_s_own() {
    let (vanilla, target) = against(REPLACE_PATH);
    let resolution = resolve(&vanilla, &target);
    let registry = row(&resolution, &trial::REPLACE_ON_REPEAT);

    assert_eq!(
        registry.keys(),
        ["tech_declarer_own"],
        "either every other source's files were not excluded, or the declarer's own file was"
    );

    // D-098 requires this row's provenance to name "the replacing declaration and every
    // excluded source", so both halves are asserted: which files went, and what took them.
    let excluded: Vec<(&str, &Removal)> = registry
        .removed_files
        .iter()
        .filter_map(|fact| match &fact.site {
            FactSite::RemovedBySelection {
                logical, removal, ..
            } => Some((logical.as_str(), removal)),
            FactSite::Stream(_) | FactSite::DeclaredDefault { .. } => None,
        })
        .collect();
    let declaration = Removal::ReplacedDirectory {
        declaration: LogicalPath::parse("common/technology").expect("a declared directory"),
    };
    assert_eq!(
        excluded,
        [
            ("common/technology/00_baseline_tech.txt", &declaration),
            ("common/technology/00_collided_tech.txt", &declaration),
        ]
    );

    // Exclusion is by directory, so a registry outside it is untouched. r3 excluded the
    // whole vanilla technology tree and nothing else.
    let events = row(&resolution, &trial::REJECT_ON_REPEAT);
    assert_eq!(events.keys(), ["notice_contested", "notice_untouched"]);
    assert!(events.removed_files.is_empty());
}

/// `r10-loadorder`, the discriminating pair.
///
/// One mod, one early-sorting filename convention, two registries whose repeat rules point
/// in opposite directions. Under a layer model the mod registers after Vanilla in both cases
/// and therefore wins the replace-on-repeat registry and loses the reject-on-repeat one.
/// The recorded outcome is the exact opposite, and asserting only one half would pass under
/// either model.
#[test]
fn r10_an_early_sorting_mod_file_wins_the_rejecting_registry_and_loses_the_replacing_one() {
    let (vanilla, target) = against(EARLY_MOD);
    let resolution = resolve(&vanilla, &target);

    let replacing = row(&resolution, &trial::REPLACE_ON_REPEAT);
    let contested = replacing
        .get("tech_contested")
        .expect("the contested key resolves");
    assert_eq!(
        contested.position.source,
        SourceKind::VanillaContent,
        "the mod won a replace-on-repeat registry from an early-sorting file, which is the \
         layer model's prediction"
    );
    assert_eq!(
        contested.position.logical.as_str(),
        "common/technology/00_baseline_tech.txt"
    );
    // The winner is the vanilla definition whole, which states `potential`.
    assert!(contested.states("potential"));

    let rejecting = row(&resolution, &trial::REJECT_ON_REPEAT);
    let notice = rejecting
        .get("notice_contested")
        .expect("the contested key resolves");
    assert_eq!(
        notice.position.source,
        SourceKind::TargetMod,
        "the mod lost a reject-on-repeat registry from an early-sorting file, which is the \
         layer model's prediction"
    );
    assert_eq!(
        notice.position.logical.as_str(),
        "events/!!!_early_notices.txt"
    );

    // Both outcomes came from one enumeration, so each records that there were two
    // registrations and names the one that lost.
    for definition in [contested, notice] {
        // The definition-level facts, filtered from the field-level ones a shadowed body
        // also produces: there were two registrations, and one of them lost.
        let kinds: Vec<FactKind> = definition
            .displaced
            .iter()
            .filter(|fact| fact.field.is_none())
            .map(|fact| fact.kind)
            .collect();
        assert_eq!(kinds, [FactKind::Duplicate, FactKind::Shadowed]);
    }
    assert_eq!(
        contested.displaced[1].site.source(),
        Some(SourceKind::TargetMod),
        "the replace-on-repeat registry shadowed the earlier mod definition"
    );
    assert_eq!(
        notice.displaced[1].site.source(),
        Some(SourceKind::VanillaContent),
        "the reject-on-repeat registry rejected the later vanilla definition"
    );
}

/// The negative control for the pair: invert either rule and the expectation must fail.
///
/// Inverted on copies of the rows rather than by editing the rows the passing test uses, so
/// the control cannot be left switched on.
#[test]
fn the_pair_fails_when_either_repeat_rule_is_inverted() {
    let (vanilla, target) = against(EARLY_MOD);
    let resolution = resolve(&vanilla, &target);

    let technologies = row(&resolution, &trial::REPLACE_SCOPE_REJECTING);
    assert_eq!(
        technologies
            .get("tech_contested")
            .expect("the contested key resolves")
            .position
            .source,
        SourceKind::TargetMod,
        "inverting the technology row's repeat rule did not change the winner, so the r10 \
         assertion is not testing the rule it claims to"
    );

    let events = row(&resolution, &trial::EVENT_SCOPE_REPLACING);
    assert_eq!(
        events
            .get("notice_contested")
            .expect("the contested key resolves")
            .position
            .source,
        SourceKind::VanillaContent,
        "inverting the event row's repeat rule did not change the winner"
    );
}

/// The r10 model's other half, stated on its own: the stream is one order over both sources.
#[test]
fn r10_provenance_records_a_position_rather_than_a_layer() {
    let (vanilla, target) = against(EARLY_MOD);
    let resolution = resolve(&vanilla, &target);
    let replacing = row(&resolution, &trial::REPLACE_ON_REPEAT);
    let contested = replacing.get("tech_contested").expect("resolves");

    // The mod's file is at stream position 0 and vanilla's at 1 — interleaved by path, with
    // the mod *before* vanilla. A layer model cannot produce that ordering at all.
    let FactSite::Stream(shadowed) = &contested.displaced[1].site else {
        panic!("a displaced definition has a stream position");
    };
    assert_eq!(
        (shadowed.order, shadowed.source),
        (0, SourceKind::TargetMod)
    );
    assert_eq!(
        (contested.position.order, contested.position.source),
        (1, SourceKind::VanillaContent)
    );
}

/// **Golden case 5**, at the resolver seam. `r1-target`.
///
/// The phase's named exit condition, and the design's first mandatory oracle case: "the
/// fixture includes a redefinition that omits `potential`, so the resolver cannot pass by
/// assuming unconditional whole-object replacement" (`docs/mvp-acceptance.md`, "Technology
/// redefinition").
///
/// Three claims held together, because each alone is satisfied by a wrong rule. One effective
/// entry per Entry Key rules out duplicate documents; the field-by-field match rules out a
/// resolver that picked the right winner and mangled it; the provenance rules out one that got
/// the answer right without being able to say where it came from.
#[test]
fn r1_a_redefinition_that_omits_potential_leaves_the_field_absent() {
    let (vanilla, target) = redefining(REDEFINITION);
    let resolution = resolve(&vanilla, &target);
    let registry = technologies(&resolution);

    // One effective entry per Entry Key. Four keys from two files that between them registered
    // six definitions — "search and browse expose one Entry Key rather than duplicate
    // documents".
    assert_eq!(
        registry.keys(),
        [
            "tech_matched_control",
            "tech_matched_subject",
            "tech_mod_only",
            "tech_untouched_baseline",
        ]
    );

    let subject = registry
        .get("tech_matched_subject")
        .expect("the subject resolves");

    // The late-sorting mod file won, which is the r1 half of the cross-source cell.
    assert_eq!(subject.position.source, SourceKind::TargetMod);
    assert_eq!(
        subject.position.logical.as_str(),
        "common/technology/zz_redefinition_tech.txt"
    );

    // Field by field, against the redefinition and nothing else. `category` is a container, so
    // it is checked by name in the shape assertion and not by value here.
    assert_eq!(
        field_names(subject),
        ["area", "cost", "tier", "category", "weight"]
    );
    assert_eq!(
        scalar_fields(subject),
        [
            ("area", "society".to_owned()),
            ("cost", "1000".to_owned()),
            ("tier", "0".to_owned()),
            ("weight", "1000000".to_owned()),
        ]
    );

    // The headline. Both fields the vanilla definition stated and the redefinition omitted are
    // absent, not inherited — `potential` because it is the recorded case, `prerequisites`
    // because one absent field could be a coincidence of how the fixture was written.
    assert!(!subject.states("potential"));
    assert!(!subject.states("prerequisites"));

    // Every effective field came from the winner. Stated over the whole registry rather than
    // the subject alone: `Inherited` appearing anywhere would mean the field rule is not the
    // one this row declares, and the row does not declare that kind, so it would refuse — this
    // assertion is what says so at the point a reader is looking for it.
    for fact in registry.provenance() {
        assert_ne!(fact.kind, FactKind::Inherited, "{fact:?}");
    }
    for field in &subject.fields {
        assert_eq!(field.kind, FactKind::Contributed);
        assert_eq!(field.site, FactSite::Stream(subject.position.clone()));
    }

    // Provenance identifies both definitions: that there were two registrations, and which one
    // lost, naming its position rather than its layer.
    let displaced: Vec<FactKind> = subject
        .displaced
        .iter()
        .filter(|fact| fact.field.is_none())
        .map(|fact| fact.kind)
        .collect();
    assert_eq!(displaced, [FactKind::Duplicate, FactKind::Shadowed]);
    let FactSite::Stream(shadowed) = &subject.displaced[1].site else {
        panic!("a displaced definition has a stream position");
    };
    assert_eq!(shadowed.source, SourceKind::VanillaContent);
    assert_eq!(
        shadowed.logical.as_str(),
        "common/technology/00_redefinition_tech.txt"
    );

    // And which facts the shadowed definition contributed — the field-level record that lets
    // documentation say *what* the redefinition removed rather than only that it removed
    // something. `potential` is in this list precisely because it is not in the effective one.
    let lost: Vec<&str> = subject
        .displaced
        .iter()
        .filter(|fact| fact.kind == FactKind::Shadowed)
        .filter_map(|fact| fact.field.as_deref())
        .collect();
    assert_eq!(
        lost,
        [
            "area",
            "cost",
            "tier",
            "category",
            "weight",
            "prerequisites",
            "potential",
        ]
    );

    // The matched control: same treatment, differing in exactly one field, so a failure names
    // the field rather than the fixture. It kept `potential`, and it is still there.
    let control = registry
        .get("tech_matched_control")
        .expect("the control resolves");
    assert_eq!(control.position.source, SourceKind::TargetMod);
    assert!(control.states("potential"));

    // The scoping control and the positive control. Without the first, "resolution broke"
    // reads exactly like "this definition was displaced"; without the second, a mod file that
    // contributed nothing at all would pass every assertion above.
    let untouched = registry
        .get("tech_untouched_baseline")
        .expect("the untouched key resolves");
    assert_eq!(untouched.position.source, SourceKind::VanillaContent);
    assert!(untouched.displaced.is_empty());
    assert_eq!(
        registry
            .get("tech_mod_only")
            .expect("the mod-only key resolves")
            .position
            .source,
        SourceKind::TargetMod
    );

    // Golden case 5's fields are compared against a pinned record, so none of them may be a
    // value the resolver deliberately left unresolved — and, since Phase 4F, none may carry
    // a scripted-constant fact either: these fixtures are all-literal, so a `ConstantFact`
    // appearing here would mean the redefinition fixtures grew a reference nobody accounted
    // for in the pinned comparison.
    for definition in registry.definitions.values() {
        assert!(definition.references.is_empty(), "{:?}", definition.key);
        assert!(definition.constants.is_empty(), "{:?}", definition.key);
    }
}

/// The negative controls for golden case 5: invert either cell and the expectation must fail.
///
/// Two cells, because golden case 5 rests on two rules and each alone is a plausible wrong
/// implementation. Inverted on copies rather than by editing the shipped row, so a control
/// cannot be left switched on.
///
/// The second is the sharper one. A first-wins technologies row produces an obviously
/// different winner; an *inheriting* one produces the same winner, the same position, the same
/// provenance, and the right answer to every assertion in golden case 5 but one — `potential`
/// comes back. That single field is the whole of the recorded result, which is why the oracle
/// fixture was built as a matched pair around it.
#[test]
fn golden_case_5_fails_when_either_the_repeat_rule_or_the_field_rule_is_inverted() {
    let (vanilla, target) = redefining(REDEFINITION);
    let resolution = resolve(&vanilla, &target);

    let rejecting = row(&resolution, &trial::REPLACE_SCOPE_REJECTING);
    let subject = rejecting
        .get("tech_matched_subject")
        .expect("the subject resolves");
    assert_eq!(
        subject.position.source,
        SourceKind::VanillaContent,
        "inverting the row's duplicate direction to first-wins did not change the winner, so \
         the r1 and r4 assertions are not testing the rule they claim to"
    );

    let inheriting = row(&resolution, &trial::TECHNOLOGY_SCOPE_INHERITING);
    let subject = inheriting
        .get("tech_matched_subject")
        .expect("the subject resolves");
    assert_eq!(
        subject.position.source,
        SourceKind::TargetMod,
        "the field rule must not change which definition wins — if it did, the control below \
         would be measuring the repeat rule a second time"
    );
    let potential = subject
        .fields
        .iter()
        .find(|field| field.field == "potential")
        .expect(
            "inheriting absent fields brings `potential` back; if it does not, the \
             omitted-`potential` assertion would pass under both field rules and prove nothing",
        );
    assert_eq!(potential.kind, FactKind::Inherited);
    assert_eq!(
        potential.site.source(),
        Some(SourceKind::VanillaContent),
        "an inherited field must name where the value actually came from"
    );
}

/// `r0-baseline`, restated: the same corpus with nothing redefining it.
///
/// The baseline is not decoration. `r0`'s draw pool contains neither the subject nor the
/// control, so `r1`'s membership is a *delta* against a measured absence rather than a reading
/// of one run. Here the same role: `potential` is present when nothing displaces the vanilla
/// definition, so its absence above is the redefinition's doing and not the fixture's.
#[test]
fn r0_the_subject_states_potential_when_nothing_redefines_it() {
    let (vanilla, target) = redefining(NO_REDEFINITION);
    let resolution = resolve(&vanilla, &target);
    let registry = technologies(&resolution);

    let subject = registry
        .get("tech_matched_subject")
        .expect("the subject resolves");
    assert_eq!(subject.position.source, SourceKind::VanillaContent);
    assert!(subject.states("potential"));
    assert!(subject.states("prerequisites"));
    assert!(
        subject.displaced.is_empty(),
        "nothing contested this key, so there is no duplicate to record"
    );
    assert!(registry.get("tech_mod_only").is_none());
}

/// `r4-reordered`'s method, applied to the technologies row: the winner follows position.
///
/// What `r4` itself did needs stating, or this test claims more than the record supports. `r4`
/// swapped the scripted-trigger and scripted-variable filenames and left
/// `common/technology/zz_oracle_tech.txt` alone — identical digest in both runs, and both
/// normalized logs name the same survivor at the same line. So for *technologies* `r4` is the
/// control its manifest says it is ("an unchanged winner means the r1 result was not a naming
/// artifact"), and the flip itself is `r1` (a late-sorting name wins) against `r10` (an early
/// one loses).
///
/// This corpus pair restates that flip directly, by `r4`'s method: the same bytes under both
/// names. The byte identity is asserted rather than trusted, because a header that drifted
/// between the two copies would quietly turn a position experiment into a content one.
#[test]
fn r4_the_winner_follows_position_and_not_content() {
    assert_eq!(
        REDEFINITION_BODY, REDEFINITION_FLIPPED_BODY,
        "the flip must vary the filename and nothing else"
    );

    let (vanilla, target) = redefining(REDEFINITION_FLIPPED);
    let resolution = resolve(&vanilla, &target);
    let registry = technologies(&resolution);

    let subject = registry
        .get("tech_matched_subject")
        .expect("the subject resolves");
    assert_eq!(
        subject.position.source,
        SourceKind::VanillaContent,
        "the same bytes that won from a late-sorting name lost from an early-sorting one, so \
         the winner is decided by position"
    );
    assert!(
        subject.states("potential"),
        "vanilla's definition won whole, and it states `potential` — the field is back because \
         a different definition won, not because anything was inherited"
    );

    // The mod file still contributed: it lost one key and won another from the same file, in
    // one enumeration. A corpus that failed to load would also produce a vanilla winner.
    assert_eq!(
        registry
            .get("tech_mod_only")
            .expect("the mod-only key resolves")
            .position
            .source,
        SourceKind::TargetMod
    );
    let FactSite::Stream(shadowed) = &subject.displaced[1].site else {
        panic!("a displaced definition has a stream position");
    };
    assert_eq!(
        (shadowed.order, shadowed.source),
        (0, SourceKind::TargetMod),
        "the mod's file is at stream position 0 and vanilla's at 1 — an ordering a layer model \
         cannot produce at all"
    );
    assert_eq!(
        (subject.position.order, subject.position.source),
        (1, SourceKind::VanillaContent)
    );
}

/// An undeclared registry refuses; it does not fall back to a neighbour's policy.
#[test]
fn an_undeclared_registry_is_a_typed_refusal() {
    let (vanilla, target) = against(EARLY_MOD);
    let resolution = resolve(&vanilla, &target);
    assert_eq!(
        resolution.registry("localization"),
        Err(Refusal::UndeclaredRegistry {
            registry: "localization".to_owned()
        }),
        "technologies is declared and localization is not, and the difference must be the \
         declaration rather than which registry the corpus happens to hold definitions for — \
         this corpus's technology files would resolve under either row's stream"
    );
}

// --- Phase 4H (STE-29): events, buildings, megastructures, and ship components ---

#[test]
fn r9_events_keep_the_first_same_source_registration_and_record_the_loser() {
    let vanilla = events_vanilla();
    let target = corpus(SourceKind::TargetMod, EVENTS);
    let registry = named(&resolve(&vanilla, &target), "events");
    let definition = registry
        .get("phase4h.same_file")
        .expect("the event resolves");
    assert_eq!(
        definition.position.ordinal, 1,
        "the first event block is retained"
    );
    assert_eq!(
        scalar_fields(definition),
        [
            ("id", "phase4h.same_file".to_owned()),
            ("title", "phase4h_same_first".to_owned())
        ]
    );
    assert_eq!(
        definition
            .displaced
            .iter()
            .filter(|fact| fact.field.is_none())
            .map(|fact| fact.kind)
            .collect::<Vec<_>>(),
        [FactKind::Duplicate, FactKind::Shadowed]
    );
}

#[test]
fn r9_and_r10_events_follow_stream_position_in_both_directions() {
    let vanilla = events_vanilla();
    let late = corpus(SourceKind::TargetMod, EVENTS);
    let late_registry = named(&resolve(&vanilla, &late), "events");
    assert_eq!(
        late_registry
            .get("phase4h.late")
            .expect("resolves")
            .position
            .source,
        SourceKind::VanillaContent,
        "a late zz_ event loses to the vanilla first registration"
    );

    let early = corpus(SourceKind::TargetMod, EVENTS_EARLY);
    let early_registry = named(&resolve(&vanilla, &early), "events");
    assert_eq!(
        early_registry
            .get("phase4h.early")
            .expect("resolves")
            .position
            .source,
        SourceKind::TargetMod,
        "an early !!!_ event wins before vanilla registers the same id"
    );
}

#[test]
fn r8_buildings_replace_the_whole_object() {
    let vanilla = buildings_vanilla();
    let target = corpus(SourceKind::TargetMod, BUILDINGS);
    let registry = named(&resolve(&vanilla, &target), "buildings");
    let subject = registry
        .get("building_phase4h_subject")
        .expect("the redefinition resolves");
    assert_eq!(subject.position.source, SourceKind::TargetMod);
    assert!(
        subject.field("building_sets").is_none(),
        "the omitted field is absent, not inherited"
    );
    assert!(
        registry.get("building_phase4h_new").is_some(),
        "the same file contributed its new-key control"
    );
}

#[test]
fn ship_components_resolve_by_inner_key_and_refuse_at_a_repeat() {
    let empty = corpus(SourceKind::VanillaContent, &[]);
    let clean = corpus(SourceKind::TargetMod, COMPONENTS);
    let clean_registry = named(&resolve(&empty, &clean), "ship-components");
    assert_eq!(
        clean_registry.keys(),
        ["PHASE4H_COMPONENT_A", "PHASE4H_COMPONENT_B"]
    );

    let repeated = corpus(SourceKind::TargetMod, COMPONENTS_REPEAT);
    assert_eq!(
        resolve(&empty, &repeated).registry("ship-components"),
        Err(Refusal::UnresolvedCell {
            registry: "ship-components",
            cell: PolicyCell::DuplicateWithinStream,
            reason: "no runtime observation names the winner of a repeated ship-component key",
            oracle_gap: "STE-22 stretch: a runtime observable for a repeated component key",
        })
    );
}

#[test]
fn ship_components_accept_file_selection_shadow_provenance_without_settling_repeats() {
    let vanilla = corpus(
        SourceKind::VanillaContent,
        &[(
            "common/component_templates/zz_phase4h_components.txt",
            b"utility_component_template = { key = \"VANILLA_SHADOWED_COMPONENT\" }",
        )],
    );
    let target = corpus(SourceKind::TargetMod, COMPONENTS);
    let registry = named(&resolve(&vanilla, &target), "ship-components");

    assert_eq!(
        registry.keys(),
        ["PHASE4H_COMPONENT_A", "PHASE4H_COMPONENT_B"],
        "the target file replaces the same-path vanilla file before component keys are read"
    );
    assert_eq!(
        registry
            .removed_files
            .iter()
            .map(|fact| fact.kind)
            .collect::<Vec<_>>(),
        [FactKind::Shadowed],
        "file selection can emit shadow provenance without reaching the pending repeat cell"
    );
}

#[test]
fn megastructures_refuse_on_the_eager_field_cell_before_a_file_is_read() {
    let vanilla = corpus(SourceKind::VanillaContent, &[]);
    let target = megastructure_target();
    let selection = super::selection::select(&vanilla, &target);
    let policy = profile::declared("megastructures").expect("the row is declared");
    assert_eq!(
        super::registry::resolve(policy, &selection, |_| {
            panic!("the eager megastructure field refusal must precede the first file read")
        }),
        Err(Refusal::UnresolvedCell {
            registry: "megastructures",
            cell: PolicyCell::FieldRule,
            reason: "r8 cannot distinguish whole replacement from inherited fields for a megastructure redefinition",
            oracle_gap: "STE-22 stretch: a runtime observable comparing a redefinition's omitted field",
        })
    );
}

#[test]
fn phase_4h_row_copy_controls_flip_the_claimed_cells() {
    const INHERITING_PROVENANCE: super::registry::ProvenanceRule =
        super::registry::ProvenanceRule {
            kinds: &[
                FactKind::Contributed,
                FactKind::Inherited,
                FactKind::Duplicate,
                FactKind::Shadowed,
            ],
        };
    const REPEATING_PROVENANCE: super::registry::ProvenanceRule = super::registry::ProvenanceRule {
        kinds: &[
            FactKind::Contributed,
            FactKind::Duplicate,
            FactKind::Shadowed,
        ],
    };
    let events = *profile::declared("events").expect("declared");
    let events_flipped = super::registry::RegistryPolicy {
        duplicates: CellStatus::Resolved(RepeatRule::ReplaceOnRepeat),
        ..events
    };
    let vanilla = events_vanilla();
    let late = corpus(SourceKind::TargetMod, EVENTS);
    assert_eq!(
        row(&resolve(&vanilla, &late), &events_flipped)
            .get("phase4h.late")
            .expect("resolves")
            .position
            .source,
        SourceKind::TargetMod,
        "inverting events changes the late winner"
    );
    let early = corpus(SourceKind::TargetMod, EVENTS_EARLY);
    assert_eq!(
        row(&resolve(&vanilla, &early), &events_flipped)
            .get("phase4h.early")
            .expect("resolves")
            .position
            .source,
        SourceKind::VanillaContent,
        "inverting events changes the early winner"
    );

    let buildings = *profile::declared("buildings").expect("declared");
    let inheriting = super::registry::RegistryPolicy {
        fields: CellStatus::Resolved(FieldRule {
            replacement: Replacement::InheritAbsentFields,
            defaults: &[],
        }),
        provenance: CellStatus::Resolved(INHERITING_PROVENANCE),
        ..buildings
    };
    let vanilla = buildings_vanilla();
    let target = corpus(SourceKind::TargetMod, BUILDINGS);
    assert!(
        row(&resolve(&vanilla, &target), &inheriting)
            .get("building_phase4h_subject")
            .expect("resolves")
            .states("building_sets")
    );

    let components = *profile::declared("ship-components").expect("declared");
    let components_resolved = super::registry::RegistryPolicy {
        duplicates: CellStatus::Resolved(RepeatRule::ReplaceOnRepeat),
        provenance: CellStatus::Resolved(REPEATING_PROVENANCE),
        ..components
    };
    let empty = corpus(SourceKind::VanillaContent, &[]);
    let repeated = corpus(SourceKind::TargetMod, COMPONENTS_REPEAT);
    assert_eq!(
        row(&resolve(&empty, &repeated), &components_resolved)
            .get("PHASE4H_COMPONENT_REPEAT")
            .expect("resolves")
            .position
            .ordinal,
        1
    );

    let mega = *profile::declared("megastructures").expect("declared");
    let mega_resolved = super::registry::RegistryPolicy {
        fields: CellStatus::Resolved(FieldRule {
            replacement: Replacement::WholeObject,
            defaults: &[],
        }),
        ..mega
    };
    let mega_target = megastructure_target();
    assert!(
        row(&resolve(&empty, &mega_target), &mega_resolved)
            .get("mega_phase4h")
            .is_some(),
        "closing the copied field cell must read and resolve the non-empty megastructure corpus"
    );
}

// --- Phase 4I (STE-30): sprite definitions ---

fn sprites(files: &[(&str, &[u8])]) -> ResolvedRegistry {
    let vanilla = sprites_vanilla();
    let target = corpus(SourceKind::TargetMod, files);
    named(&resolve(&vanilla, &target), "sprites")
}

fn texture(definition: &ResolvedDefinition) -> (&str, &FactSite) {
    let resolution = definition
        .sprite
        .as_ref()
        .expect("a sprite definition carries sprite resolution");
    let SpriteTextureOutcome::Resolved(texture) = &resolution.texture else {
        panic!(
            "expected a resolved sprite texture: {:?}",
            resolution.texture
        );
    };
    (&texture.path, &texture.site)
}

fn r17_dependents_use_mod_texture(registry: &ResolvedRegistry) -> bool {
    ["GFX_alert_first", "GFX_alert_second"]
        .into_iter()
        .all(|key| {
            registry.get(key).is_some_and(|definition| {
                let (path, site) = texture(definition);
                path == "gfx/interface/icons/phase4i_mod.dds"
                    && site.source() == Some(SourceKind::TargetMod)
            })
        })
}

#[test]
fn r17_sprites_replace_last_within_files_across_files_and_sources() {
    let registry = sprites(SPRITES);

    let same_file = registry
        .get("GFX_phase4i_same_file")
        .expect("the same-file key resolves");
    assert_eq!(same_file.position.ordinal, 1);
    assert_eq!(
        texture(same_file).0,
        "gfx/interface/icons/phase4i_same_file_last.dds"
    );

    let cross_file = registry
        .get("GFX_phase4i_cross_file")
        .expect("the cross-file key resolves");
    assert_eq!(
        cross_file.position.logical.as_str(),
        "interface/zz_phase4i_sprites_b.gfx"
    );
    assert_eq!(
        texture(cross_file).0,
        "gfx/interface/icons/phase4i_cross_file_last.dds"
    );

    let sheet = registry.get("GFX_alerticons").expect("the sheet resolves");
    assert_eq!(sheet.position.source, SourceKind::TargetMod);
    assert_eq!(texture(sheet).0, "gfx/interface/icons/phase4i_mod.dds");
    assert_eq!(
        sheet
            .displaced
            .iter()
            .filter(|fact| fact.field.is_none())
            .map(|fact| fact.kind)
            .collect::<Vec<_>>(),
        [FactKind::Duplicate, FactKind::Shadowed],
        "the winning definition retains the contested registration and its displaced body"
    );
    assert!(
        sheet.displaced.iter().any(|fact| {
            fact.kind == FactKind::Shadowed
                && fact.field.as_deref() == Some("texturefile")
                && fact.site.source() == Some(SourceKind::VanillaContent)
        }),
        "the shadowed Vanilla texture remains field-level provenance"
    );
}

#[test]
fn r17_dependents_record_the_edge_and_the_texture_s_actual_source() {
    let registry = sprites(SPRITES);
    assert!(r17_dependents_use_mod_texture(&registry));

    for key in ["GFX_alert_first", "GFX_alert_second"] {
        let dependent = registry.get(key).expect("the dependent resolves");
        assert_eq!(
            dependent.position.source,
            SourceKind::VanillaContent,
            "the referring sprite is still Vanilla-owned"
        );
        let resolution = dependent.sprite.as_ref().expect("sprite resolution");
        let edge = resolution.references.first().expect("the sheet edge");
        assert_eq!(edge.sprite.as_deref(), Some("GFX_alerticons"));
        assert_eq!(edge.site.source(), Some(SourceKind::VanillaContent));
        assert_eq!(
            edge.target.as_ref().and_then(FactSite::source),
            Some(SourceKind::TargetMod),
            "the edge names the winning sheet definition"
        );
        let SpriteTextureOutcome::Resolved(texture) = &edge.outcome else {
            panic!("the edge resolves: {:?}", edge.outcome);
        };
        assert_eq!(texture.path, "gfx/interface/icons/phase4i_mod.dds");
        assert_eq!(texture.site.source(), Some(SourceKind::TargetMod));
    }
}

#[test]
fn r18_an_early_sprite_override_loses_by_path_position() {
    let registry = sprites(SPRITES_EARLY);
    let sheet = registry.get("GFX_alerticons").expect("the sheet resolves");
    assert_eq!(sheet.position.source, SourceKind::VanillaContent);
    assert_eq!(texture(sheet).0, "gfx/interface/icons/phase4i_vanilla.dds");
    assert!(sheet.displaced.iter().any(|fact| {
        fact.kind == FactKind::Shadowed
            && fact.field.as_deref() == Some("texturefile")
            && fact.site.source() == Some(SourceKind::TargetMod)
    }));

    for key in ["GFX_alert_first", "GFX_alert_second"] {
        let (path, site) = texture(registry.get(key).expect("the dependent resolves"));
        assert_eq!(path, "gfx/interface/icons/phase4i_vanilla.dds");
        assert_eq!(site.source(), Some(SourceKind::VanillaContent));
    }
}

#[test]
fn sprite_reference_chains_and_failures_are_total_and_typed() {
    let registry = sprites(SPRITES);

    let chain = registry
        .get("GFX_phase4i_chain_start")
        .expect("the chain resolves")
        .sprite
        .as_ref()
        .expect("sprite resolution");
    let SpriteTextureOutcome::Resolved(texture) = &chain.texture else {
        panic!("the two-hop chain resolves: {:?}", chain.texture);
    };
    assert_eq!(texture.path, "gfx/interface/icons/phase4i_mod.dds");
    assert_eq!(texture.site.source(), Some(SourceKind::TargetMod));
    assert_eq!(
        chain
            .references
            .iter()
            .map(|edge| edge.sprite.as_deref())
            .collect::<Vec<_>>(),
        [Some("GFX_phase4i_chain_middle"), Some("GFX_alerticons")]
    );

    let missing_target = registry
        .get("GFX_phase4i_missing_target")
        .expect("the definition survives")
        .sprite
        .as_ref()
        .expect("sprite resolution");
    assert_eq!(
        missing_target.texture,
        SpriteTextureOutcome::MissingTarget {
            sprite: Some("GFX_phase4i_absent".to_owned())
        }
    );

    let missing_texture = registry
        .get("GFX_phase4i_missing_texture")
        .expect("the definition survives")
        .sprite
        .as_ref()
        .expect("sprite resolution");
    assert_eq!(
        missing_texture.texture,
        SpriteTextureOutcome::MissingTexture
    );

    let cycle = registry
        .get("GFX_phase4i_cycle_a")
        .expect("the definition survives")
        .sprite
        .as_ref()
        .expect("sprite resolution");
    assert_eq!(
        cycle.texture,
        SpriteTextureOutcome::CyclicReference {
            sprite: "GFX_phase4i_cycle_a".to_owned()
        }
    );
    assert_eq!(cycle.references.len(), 2);
    assert!(
        cycle
            .references
            .iter()
            .all(|edge| edge.outcome == cycle.texture)
    );
}

#[test]
fn resolving_sheet_edges_against_the_shadowed_definition_breaks_r17() {
    let mut late = sprites(SPRITES);
    assert!(r17_dependents_use_mod_texture(&late));

    let early = sprites(SPRITES_EARLY);
    let (wrong_key, wrong_sheet) = early
        .definitions
        .iter()
        .find(|(key, _)| key.as_str() == "GFX_alerticons")
        .expect("the Vanilla sheet wins the early corpus");
    late.definitions
        .insert(wrong_key.clone(), wrong_sheet.clone());
    super::sprites::attach(&mut late.definitions);

    assert!(
        !r17_dependents_use_mod_texture(&late),
        "the r17 assertion must fail when a dependent consults the shadowed Vanilla sheet \
         instead of the final Target Mod winner"
    );
}

// --- Phase 4F (STE-27): scripted triggers, effects, and constants ---

/// `r1-target`'s trigger and effect half, restated: last in enumeration order wins, both
/// within one file and across two.
#[test]
fn r1_scripted_triggers_and_effects_replace_on_repeat_last_wins() {
    let (vanilla, target) = registration();
    let resolution = resolve(&vanilla, &target);

    let triggers = named(&resolution, "scripted-triggers");
    let same_file = triggers
        .get("trig_same_file")
        .expect("the same-file key resolves");
    assert_eq!(
        same_file.position.ordinal, 2,
        "the last registration in the file wins"
    );
    assert_eq!(
        scalar_fields(same_file),
        [("always", "no".to_owned())],
        "the always = no body is the winner"
    );
    let cross_file = triggers
        .get("trig_cross_file")
        .expect("the cross-file key resolves");
    assert_eq!(
        cross_file.position.logical.as_str(),
        "common/scripted_triggers/zz_dup_triggers_b.txt",
        "the later-sorting file wins the cross-file repeat"
    );
    // Shadowed bodies never contribute effective fields — only the winner's do.
    assert_eq!(scalar_fields(cross_file), [("always", "no".to_owned())]);

    let effects = named(&resolution, "scripted-effects");
    let eff = effects
        .get("eff_same_file")
        .expect("the effect key resolves");
    assert_eq!(eff.position.ordinal, 1, "the last registration wins");
    assert_eq!(
        scalar_fields(eff),
        [("set_country_flag", "flag_b".to_owned())]
    );
    let definition_level_shadowed = eff
        .displaced
        .iter()
        .filter(|fact| fact.kind == FactKind::Shadowed && fact.field.is_none())
        .count();
    assert_eq!(
        definition_level_shadowed, 1,
        "duplicates do not accumulate: one repeat, one definition-level Shadowed fact"
    );
}

/// `r1-target`'s constant half, restated: first in enumeration order wins — the opposite
/// direction from triggers and effects.
#[test]
fn r1_scripted_constants_reject_on_repeat_first_wins() {
    let (vanilla, target) = registration();
    let resolution = resolve(&vanilla, &target);
    let constants = named(&resolution, "scripted-constants");

    let same_file = constants
        .get("@const_same_file")
        .expect("the same-file key resolves");
    assert_eq!(
        same_file.position.ordinal, 0,
        "the first registration in the file wins"
    );

    let cross_file = constants
        .get("@const_cross_file")
        .expect("the cross-file key resolves");
    assert_eq!(
        cross_file.position.logical.as_str(),
        "common/scripted_variables/zz_dup_constants_a.txt",
        "the earlier-sorting file wins the cross-file repeat"
    );

    for key in ["@const_same_file", "@const_cross_file"] {
        let definition = constants.get(key).expect("resolves");
        let definition_level: Vec<FactKind> = definition
            .displaced
            .iter()
            .filter(|fact| fact.field.is_none())
            .map(|fact| fact.kind)
            .collect();
        assert_eq!(
            definition_level,
            [FactKind::Duplicate, FactKind::Shadowed],
            "{key}"
        );
    }
}

/// `r1`'s consumption cases through the technologies row: a file-local override, a vanilla
/// cross-source read, and a cross-file-won constant.
#[test]
fn r1_the_technologies_row_consumes_registered_constants() {
    let (vanilla, target) = registration();
    let resolution = resolve(&vanilla, &target);
    let registry = technologies(&resolution);

    let local = registry
        .get("tech_local_consumer")
        .expect("resolves")
        .constants
        .first()
        .expect("a constant fact")
        .clone();
    let ConstantOutcome::Resolved { value, declaration } = &local.outcome else {
        panic!(
            "expected tech_local_consumer to resolve: {:?}",
            local.outcome
        );
    };
    assert_eq!(
        value.value(),
        crate::canonical::numeric::SourceNumber::parse("99").value()
    );
    assert_eq!(
        declaration.source(),
        Some(SourceKind::TargetMod),
        "the declaration site names the consuming file's own local declaration"
    );
    assert_eq!(
        declaration.logical().map(LogicalPath::as_str),
        Some("common/technology/zz_consumer_tech.txt")
    );

    let global = registry
        .get("tech_global_consumer")
        .expect("resolves")
        .constants
        .first()
        .expect("a constant fact")
        .clone();
    let ConstantOutcome::Resolved { value, .. } = &global.outcome else {
        panic!(
            "expected tech_global_consumer to resolve: {:?}",
            global.outcome
        );
    };
    assert_eq!(
        value.value(),
        crate::canonical::numeric::SourceNumber::parse("10").value(),
        "the cross-file constant winner (the earlier-sorting file) is what this consumer sees"
    );

    let from_vanilla = registry
        .get("tech_vanilla_consumer")
        .expect("resolves")
        .constants
        .first()
        .expect("a constant fact")
        .clone();
    let ConstantOutcome::Resolved { value, declaration } = &from_vanilla.outcome else {
        panic!(
            "expected tech_vanilla_consumer to resolve: {:?}",
            from_vanilla.outcome
        );
    };
    assert_eq!(
        value.value(),
        crate::canonical::numeric::SourceNumber::parse("20").value()
    );
    assert_eq!(
        declaration.source(),
        Some(SourceKind::VanillaContent),
        "the vanilla constant read from a mod file resolves, and names Vanilla as its source"
    );
}

/// `r4-reordered`'s method, applied to the trigger and constant pairs: the two file names in
/// each pair are exchanged with each other, and the cross-file winner is expected to move
/// with the name — the opposite-directions proof, restated for Phase 4F's own rows.
#[test]
fn r4_the_trigger_and_constant_winners_follow_position_and_not_content() {
    // The flip must vary the filename and nothing else; assert the byte identity of the
    // swapped pairs before trusting what moves.
    assert_eq!(TRIGGERS_A_BODY, TRIGGERS_B_FLIPPED_BODY);
    assert_eq!(TRIGGERS_B_BODY, TRIGGERS_A_FLIPPED_BODY);
    assert_eq!(CONSTANTS_A_BODY, CONSTANTS_B_FLIPPED_BODY);
    assert_eq!(CONSTANTS_B_BODY, CONSTANTS_A_FLIPPED_BODY);

    let (vanilla, target) = registration_flipped();
    let resolution = resolve(&vanilla, &target);

    let triggers = named(&resolution, "scripted-triggers");
    let same_file = triggers.get("trig_same_file").expect("resolves");
    assert_eq!(
        scalar_fields(same_file),
        [("always", "no".to_owned())],
        "same-file results are unaffected by the cross-file flip"
    );
    let cross_file = triggers.get("trig_cross_file").expect("resolves");
    assert_eq!(
        cross_file.position.logical.as_str(),
        "common/scripted_triggers/zz_dup_triggers_b.txt",
        "the winner is still the LAST file in stream order — its name never moved, only the \
         content living inside it did"
    );
    assert_eq!(
        scalar_fields(cross_file),
        [("always", "yes".to_owned())],
        "the content that swapped into the last-sorting file is now the winner's value"
    );

    let constants = named(&resolution, "scripted-constants");
    let same_file = constants.get("@const_same_file").expect("resolves");
    assert_eq!(
        same_file.position.ordinal, 0,
        "same-file results are unaffected by the flip"
    );
    let cross_file = constants.get("@const_cross_file").expect("resolves");
    assert_eq!(
        cross_file.position.logical.as_str(),
        "common/scripted_variables/zz_dup_constants_a.txt",
        "the winner is still the FIRST file in stream order, which now holds the other content"
    );
    // The constants row's own declaration facts have `field: None`.
    let fact = cross_file.constants.first().expect("a declaration fact");
    let ConstantOutcome::Resolved { value, .. } = &fact.outcome else {
        panic!("expected @const_cross_file to resolve: {:?}", fact.outcome);
    };
    assert_eq!(
        value.value(),
        crate::canonical::numeric::SourceNumber::parse("20").value(),
        "the value moved with the name, from 10 to 20"
    );
}

/// `r5-risky-constants` and `r7-risky-consumed`, through the technologies row: a forward
/// reference and a two-symbol cycle each carry a typed failure, never a fabricated value.
///
/// `r7`'s evidence is why detection is resolver-owned rather than read off the game's log:
/// consuming `@fwd` corrupts the file with `unknown command 'tier' for MTTH/script value`,
/// a diagnostic that never names the broken constant.
#[test]
fn r5_and_r7_a_forward_reference_and_a_cycle_never_present_a_fabricated_value() {
    let (vanilla, target) = risky_constants();
    let resolution = resolve(&vanilla, &target);
    let registry = technologies(&resolution);

    for (key, expected) in [
        (
            "tech_fwd_consumer",
            UnresolvedConstant::DeclarationNeverResolves,
        ),
        (
            "tech_cycle_consumer",
            UnresolvedConstant::DeclarationNeverResolves,
        ),
        (
            "tech_undeclared_consumer",
            UnresolvedConstant::UndeclaredSymbol,
        ),
    ] {
        let definition = registry.get(key).expect("resolves");
        assert_eq!(
            definition.constants.len(),
            1,
            "{key}: {:?}",
            definition.constants
        );
        let fact = &definition.constants[0];
        assert_eq!(fact.field.as_deref(), Some("cost"), "{key}");
        assert_eq!(
            fact.outcome,
            ConstantOutcome::Unresolved(expected),
            "{key} must never carry a fabricated value"
        );
        // The reference text is still what `cost` states — never a hole and never a number.
        assert!(definition.states("cost"), "{key}");
    }

    // The constants row's own declarations carry the same failure, with `field: None`: the
    // fact is about the declaration's own value, not about something that read it.
    let constants = named(&resolution, "scripted-constants");
    for symbol in ["@fwd", "@cycle_a", "@cycle_b"] {
        let definition = constants.get(symbol).expect("resolves");
        assert_eq!(definition.constants.len(), 1, "{symbol}");
        let fact = &definition.constants[0];
        assert_eq!(fact.field, None, "{symbol}");
        assert_eq!(
            fact.outcome,
            ConstantOutcome::Unresolved(UnresolvedConstant::DeclarationNeverResolves),
            "{symbol}"
        );
    }
    // The control: `@fwd_target` is a plain literal and resolves, so the failures above are
    // about reference direction and not about the fixture.
    let fwd_target = constants.get("@fwd_target").expect("resolves");
    let fact = fwd_target.constants.first().expect("a declaration fact");
    assert!(matches!(fact.outcome, ConstantOutcome::Resolved { .. }));
}

/// The scripted-constants cross-source open cell. Asking the shipped row for itself by name
/// refuses wholesale on the collision; the same row over a same-source corpus resolves.
#[test]
fn the_scripted_constants_cross_source_cell_refuses_only_on_a_collision() {
    let (vanilla, target) = constants_collision();
    let resolution = resolve(&vanilla, &target);
    assert_eq!(
        resolution.registry("scripted-constants"),
        Err(Refusal::UnresolvedCell {
            registry: "scripted-constants",
            cell: PolicyCell::CrossSourceCollision,
            reason: "no record measures a scripted-constant repeat spanning Vanilla and the \
                     Target Mod",
            oracle_gap: "the next capture, r19: a run redefining a vanilla scripted constant \
                         from an early-sorting Target Mod file",
        })
    );

    let (vanilla, target) = registration();
    let resolution = resolve(&vanilla, &target);
    assert!(
        resolution.registry("scripted-constants").is_ok(),
        "no symbol in this corpus repeats across sources, so the row resolves"
    );
}

/// The consuming side of the same open cell: one contested symbol is pending, one clean
/// symbol still resolves, from the same corpus and the same technologies row.
#[test]
fn the_technologies_row_marks_only_the_colliding_symbol_pending() {
    let (vanilla, target) = constants_collision();
    let resolution = resolve(&vanilla, &target);
    let registry = technologies(&resolution);

    let collision = registry.get("tech_collision_consumer").expect("resolves");
    assert_eq!(
        collision.constants.first().map(|fact| &fact.outcome),
        Some(&ConstantOutcome::Unresolved(
            UnresolvedConstant::CrossSourcePending
        ))
    );

    let clean = registry.get("tech_clean_consumer").expect("resolves");
    let ConstantOutcome::Resolved { .. } =
        &clean.constants.first().expect("a constant fact").outcome
    else {
        panic!("a clean symbol must still resolve while a colliding one is pending");
    };
}

/// The `$PARAM$` reference open cell: the shipped scripted-triggers row refuses only once a
/// definition actually carries a parameter substitution.
#[test]
fn the_parameter_open_cell_refuses_only_when_a_definition_carries_one() {
    let (vanilla, target) = parameterized();
    let resolution = resolve(&vanilla, &target);
    assert_eq!(
        resolution.registry("scripted-triggers"),
        Err(Refusal::UnresolvedCell {
            registry: "scripted-triggers",
            cell: PolicyCell::UnresolvedReferences,
            reason: "no record measures $PARAM$ substitution in a trigger or effect body",
            oracle_gap: "a capture exercising a parameterised scripted trigger/effect call",
        })
    );

    let (vanilla, target) = registration();
    let resolution = resolve(&vanilla, &target);
    assert!(
        resolution.registry("scripted-triggers").is_ok(),
        "this corpus's triggers carry no $PARAM$ substitution"
    );
}

/// Direction negative controls, on row copies rather than the shipped rows: inverting either
/// the triggers rule or the constants rule over `registration/` produces the opposite winner.
#[test]
fn inverting_either_direction_produces_the_opposite_winner() {
    let (vanilla, target) = registration();
    let resolution = resolve(&vanilla, &target);

    let triggers = row(&resolution, &trial::TRIGGERS_SCOPE_REJECTING);
    assert_eq!(
        triggers
            .get("trig_cross_file")
            .expect("resolves")
            .position
            .logical
            .as_str(),
        "common/scripted_triggers/zz_dup_triggers_a.txt",
        "inverting the triggers row to reject-on-repeat must move the winner to the FIRST file"
    );

    let constants = row(&resolution, &trial::CONSTANTS_ROW_REPLACING);
    assert_eq!(
        constants
            .get("@const_cross_file")
            .expect("resolves")
            .position
            .logical
            .as_str(),
        "common/scripted_variables/zz_dup_constants_b.txt",
        "inverting the constants row to replace-on-repeat must move the winner to the LAST file"
    );
}

// --- Phase 4G (STE-28): inline-script expansion ---

/// `inline-vanilla/` paired with `inline/`: `r11-inline`'s six subjects, restated.
fn inline() -> (SourceSnapshot, SourceSnapshot) {
    (inline_vanilla(), corpus(SourceKind::TargetMod, INLINE))
}

/// `r12-inline-missing`, restated.
fn inline_missing() -> (SourceSnapshot, SourceSnapshot) {
    (
        inline_vanilla(),
        corpus(SourceKind::TargetMod, INLINE_MISSING),
    )
}

/// The scalar `key = value` pairs of every `modifier` block inside a definition's effective
/// `weight_modifier`.
///
/// `r11` made every subject expand to one shape so that draw-pool membership read the same
/// way for all of them. This is that comparison at the resolver seam: each subject's result is
/// compared against the hand-written literal control's, so "expanded" is measured against a
/// known-good value rather than against a shape the assertion invented.
fn weight_modifier_blocks(definition: &ResolvedDefinition) -> Vec<Vec<(String, String)>> {
    let Some(Value::Container(container)) = definition.field("weight_modifier") else {
        return Vec::new();
    };
    container
        .fields()
        .filter(|field| field.key.text() == "modifier")
        .map(|field| match &field.value {
            Value::Container(inner) => inner
                .fields()
                .filter_map(|nested| match &nested.value {
                    Value::Scalar(scalar) => {
                        Some((nested.key.text().into_owned(), scalar.text().into_owned()))
                    }
                    Value::Container(_) | Value::Tagged { .. } => None,
                })
                .collect(),
            Value::Scalar(_) | Value::Tagged { .. } => Vec::new(),
        })
        .collect()
}

/// The hand-written positive control's expanded shape, read from the corpus rather than
/// written out here: an assertion that restated the expected literal would pass even if the
/// control itself drifted, and then it would no longer be a control.
fn literal_control(registry: &ResolvedRegistry) -> Vec<Vec<(String, String)>> {
    let control = weight_modifier_blocks(
        registry
            .get("tech_inline_literal")
            .expect("the positive control resolves"),
    );
    assert_eq!(
        control,
        [[
            ("factor".to_owned(), "1000000".to_owned()),
            ("always".to_owned(), "yes".to_owned()),
        ]],
        "the positive control names no inline script, so this shape is what the corpus states \
         outright — if it has drifted, every comparison below is against the wrong thing"
    );
    control
}

/// The two controls that bracket every expansion result: the literal subject states the target
/// shape outright, and the no-modifier subject shows what "the mechanism did nothing" looks
/// like. Without the second, an expansion producing an empty `weight_modifier` would be
/// indistinguishable from one that worked.
#[test]
fn r11_the_literal_and_no_modifier_controls_bracket_every_expansion() {
    let (vanilla, target) = inline();
    let resolution = resolve(&vanilla, &target);
    let registry = technologies(&resolution);

    let control = literal_control(&registry);
    assert!(
        registry
            .get("tech_inline_literal")
            .expect("resolves")
            .inline_expansions
            .is_empty(),
        "the literal control names no inline script, so it has no expansion site to record"
    );

    let nothing = registry
        .get("tech_inline_lowweight")
        .expect("the negative control resolves");
    assert!(
        !nothing.states("weight_modifier"),
        "the negative control states no modifier of any kind"
    );
    assert_ne!(
        weight_modifier_blocks(nothing),
        control,
        "the two controls must differ, or neither discriminates"
    );

    // The scoping control from the base-game side: a definition naming no inline script at
    // all survives expansion untouched.
    let baseline = registry
        .get("tech_inline_baseline")
        .expect("the base-game definition resolves");
    assert_eq!(baseline.position.source, SourceKind::VanillaContent);
    assert!(baseline.inline_expansions.is_empty());
    assert!(baseline.states("weight"));
}

/// `r11`'s first subject: does a simple inclusion expand at all?
#[test]
fn r11_a_simple_inline_script_expands_into_the_consuming_definition() {
    let (vanilla, target) = inline();
    let resolution = resolve(&vanilla, &target);
    let registry = technologies(&resolution);
    let control = literal_control(&registry);

    let subject = registry.get("tech_inline_basic").expect("resolves");
    assert_eq!(
        weight_modifier_blocks(subject),
        control,
        "the fragment's content must be spliced in where the site was"
    );
    assert!(
        subject.references.is_empty(),
        "an expanded inclusion is settled, not an unfinished value: {:?}",
        subject.references
    );

    let fact = subject
        .inline_expansions
        .first()
        .expect("the site is recorded");
    assert_eq!(fact.reference.as_deref(), Some("oracle/factor_block"));
    assert_eq!(fact.field, "weight_modifier");
    let InlineOutcome::Expanded { script, bindings } = &fact.outcome else {
        panic!("expected expansion: {:?}", fact.outcome);
    };
    assert_eq!(
        script.logical().map(LogicalPath::as_str),
        Some("common/inline_scripts/oracle/factor_block.txt"),
        "the resolved source path is what the resolution matrix requires of every site"
    );
    assert!(bindings.is_empty());
    assert_eq!(script.source(), Some(SourceKind::TargetMod));
}

/// `r11`'s second subject: `$PARAM$` substitution, and the bindings recorded beside it.
#[test]
fn r11_parameters_substitute_into_the_expanded_content() {
    let (vanilla, target) = inline();
    let resolution = resolve(&vanilla, &target);
    let registry = technologies(&resolution);
    let control = literal_control(&registry);

    let subject = registry.get("tech_inline_param").expect("resolves");
    assert_eq!(
        weight_modifier_blocks(subject),
        control,
        "`factor = $F$` with F bound to 1000000 must produce the control's shape exactly — a \
         substituter that dropped the token would leave a factor of nothing behind"
    );

    let fact = subject
        .inline_expansions
        .first()
        .expect("the site is recorded");
    assert_eq!(fact.reference.as_deref(), Some("oracle/param_factor"));
    let InlineOutcome::Expanded { bindings, .. } = &fact.outcome else {
        panic!("expected expansion: {:?}", fact.outcome);
    };
    assert_eq!(
        bindings,
        &[("F".to_owned(), "1000000".to_owned())],
        "the parameter bindings are required provenance, not an implementation detail"
    );
}

/// `r11`'s third subject: an inclusion inside an inclusion.
///
/// The recursion depth that matters. Vanilla inline scripts contain inline scripts, the game
/// expands them correctly, and an expander handling one level would drop the content with
/// nothing logged anywhere — which is why this is the ticket's named negative control.
#[test]
fn r11_nested_inclusion_expands_recursively() {
    let (vanilla, target) = inline();
    let resolution = resolve(&vanilla, &target);
    let registry = technologies(&resolution);
    let control = literal_control(&registry);

    let subject = registry.get("tech_inline_nested").expect("resolves");
    assert_eq!(
        weight_modifier_blocks(subject),
        control,
        "the outer fragment states nothing but a second inclusion, so this shape can only \
         come from expanding the inner one too"
    );

    let sites: Vec<(Option<&str>, Option<&str>)> = subject
        .inline_expansions
        .iter()
        .map(|fact| {
            (
                fact.reference.as_deref(),
                fact.site.logical().map(LogicalPath::as_str),
            )
        })
        .collect();
    assert_eq!(
        sites,
        [
            (
                Some("oracle/outer_factor"),
                Some("common/technology/zz_inline_nested.txt")
            ),
            (
                Some("oracle/inner_factor"),
                Some("common/inline_scripts/oracle/outer_factor.txt")
            ),
        ],
        "each site is recorded where it is actually written — the nested one in the fragment \
         that states it, not in the consuming technology"
    );
}

/// `r11`'s fourth subject: a mod file at a vanilla inline script's path.
///
/// This falls out of step 1 rather than any registration rule — inline scripts have no
/// declared identifier to collide on, so their only collision mode is `r6`'s exact-path
/// replacement, applied before any stream exists.
#[test]
fn r11_a_mod_file_at_a_vanilla_script_s_path_overrides_its_content() {
    let (vanilla, target) = inline();
    let resolution = resolve(&vanilla, &target);
    let registry = technologies(&resolution);
    let control = literal_control(&registry);

    let subject = registry
        .get("tech_inline_override_probe")
        .expect("resolves");
    assert_eq!(
        weight_modifier_blocks(subject),
        control,
        "the base-game body at this path is gated and would not produce the control's shape, \
         so this result can only come from the mod's file having won the path"
    );

    let fact = subject
        .inline_expansions
        .first()
        .expect("the site is recorded");
    let InlineOutcome::Expanded { script, bindings } = &fact.outcome else {
        panic!("expected expansion: {:?}", fact.outcome);
    };
    assert_eq!(
        script.source(),
        Some(SourceKind::TargetMod),
        "provenance must name the source that actually supplied the expanded content"
    );
    assert_eq!(
        script.logical().map(LogicalPath::as_str),
        Some("common/inline_scripts/technologies/rare_weight_modifiers.txt")
    );
    assert_eq!(
        bindings,
        &[(
            "TECHNOLOGY".to_owned(),
            "tech_inline_override_probe".to_owned()
        )],
        "the fragment never references this binding, and `r11` shows the game accepts it — \
         an unused binding is a fact about the call, not a fault"
    );
}

/// `r12-inline-missing`: the reference does not resolve, and the definition survives anyway.
///
/// The record's point is that this failure is *quiet*. Unlike a broken scripted constant
/// (`r7`), nothing downstream is corrupted: the technology registers, structurally valid, with
/// the included content simply absent. So the resolver owes the same survival plus an explicit
/// fact, because otherwise "failed to expand" and "there was nothing to expand" are one
/// silence.
#[test]
fn r12_an_unresolved_inline_reference_is_a_typed_fact_and_the_definition_survives() {
    let (vanilla, target) = inline_missing();
    let resolution = resolve(&vanilla, &target);
    // Asked by name through the public seam: a refusal here would be the resolver taking the
    // whole registry down over a fault the game itself walks away from.
    let registry = technologies(&resolution);

    let subject = registry
        .get("tech_missing_inline")
        .expect("the definition still registers");
    assert_eq!(
        field_names(subject),
        ["area", "cost", "tier", "weight", "weight_modifier"],
        "every field survives, including the one that held the inclusion"
    );
    assert!(
        weight_modifier_blocks(subject).is_empty(),
        "the inclusion is absent from the field it would have filled"
    );

    let fact = subject
        .inline_expansions
        .first()
        .expect("the failure is recorded rather than silent");
    assert_eq!(
        fact.reference.as_deref(),
        Some("oracle/this_inline_script_does_not_exist"),
        "the fact must name the path that did not resolve — the game's own diagnostic names \
         it, and a fact that did not could not be surfaced as an Analysis Issue"
    );
    assert_eq!(fact.field, "weight_modifier");
    assert_eq!(
        fact.outcome,
        InlineOutcome::Unresolved(UnresolvedInline::UnknownPath)
    );
    assert!(
        subject.references.is_empty(),
        "an omitted inclusion is settled with a reason, not left as an unfinished value"
    );

    // The isolation control: the definition after the failing one in the same file resolves
    // normally, which is the difference between this and `r7`'s cascade.
    let sibling = registry
        .get("tech_missing_sibling")
        .expect("the following definition is unaffected");
    assert!(sibling.inline_expansions.is_empty());
    assert!(sibling.states("weight"));
}
