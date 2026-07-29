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
//! Three run in ordinary CI as tests of their own:
//! [`the_pair_fails_when_either_repeat_rule_is_inverted`],
//! [`merge_by_key_would_keep_the_shadowed_file_s_other_keys`], and `record::tests`.
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

mod record;

use super::registry::Refusal;
use super::resolved::{FactKind, FactSite, Removal, ResolvedRegistry};
use super::trial::{self, EARLY_MOD, PATH_COLLISION, REPLACE_PATH, corpus, vanilla};
use super::{Resolution, profile, resolve};
use crate::canonical::path::LogicalPath;
use crate::source::SourceKind;

/// One captured run this suite holds the resolver to.
struct Expectation {
    run: &'static str,
    /// What the record established, in the words the evaluation uses for it.
    rule: &'static str,
}

/// The records the Phase 4D core consumes. Row tickets add their own.
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

/// An undeclared registry refuses; it does not fall back to a neighbour's policy.
#[test]
fn an_undeclared_registry_is_a_typed_refusal() {
    let (vanilla, target) = against(EARLY_MOD);
    let resolution = resolve(&vanilla, &target);
    assert_eq!(
        resolution.registry("technologies"),
        Err(Refusal::UndeclaredRegistry {
            registry: "technologies".to_owned()
        }),
        "the profile declares no rows yet, so every registry must refuse by name"
    );
}
