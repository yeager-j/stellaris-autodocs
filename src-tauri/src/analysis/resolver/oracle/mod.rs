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

mod record;

use super::registry::Refusal;
use super::resolved::{FactKind, FactSite, Removal, ResolvedDefinition, ResolvedRegistry};
use super::trial::{
    self, EARLY_MOD, NO_REDEFINITION, PATH_COLLISION, REDEFINITION, REDEFINITION_BODY,
    REDEFINITION_FLIPPED, REDEFINITION_FLIPPED_BODY, REPLACE_PATH, corpus, redefinition_vanilla,
    vanilla,
};
use super::{Resolution, profile, resolve};
use crate::analysis::parser::Value;
use crate::canonical::path::LogicalPath;
use crate::source::{SourceKind, SourceSnapshot};

/// One captured run this suite holds the resolver to.
struct Expectation {
    run: &'static str,
    /// What the record established, in the words the evaluation uses for it.
    rule: &'static str,
}

/// The records consumed so far: the Phase 4D core's three, then the technologies row's.
///
/// Each row ticket appends its own. A record listed here is under the drift gate whether or
/// not an expectation below reads it in detail — `r0-baseline` is the clearest case, since its
/// value is the measured *absence* the r1 result is a delta against.
const EXPECTATIONS: &[Expectation] = &[
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
    // value the resolver deliberately left unresolved.
    for definition in registry.definitions.values() {
        assert!(definition.references.is_empty(), "{:?}", definition.key);
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
        resolution.registry("megastructures"),
        Err(Refusal::UndeclaredRegistry {
            registry: "megastructures".to_owned()
        }),
        "technologies is declared and megastructures is not, and the difference must be the \
         declaration rather than which registry the corpus happens to hold definitions for — \
         this corpus's technology files would resolve under either row's stream"
    );
}
