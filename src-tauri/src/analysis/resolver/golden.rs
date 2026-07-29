//! Golden-case fixture expectations that do not rest on a captured oracle record.
//!
//! [`super::oracle`] holds claims about what the *game* does, each one anchored to a run under
//! `docs/spikes/oracle-records/` and gated against the pinned build. These three fixtures are a
//! different kind of evidence. `docs/mvp-acceptance.md`'s golden cases 2, 3, and 4 describe
//! shapes a mod corpus can have — a `×0` weight modifier, a zero base Draw Weight fed by a
//! constant and granted from two enclosing actions, a corpus that partly fails to parse — and
//! nothing about those shapes needs a game observation to be worth committing. Filing them
//! under `oracle` would imply an anchor that does not exist.
//!
//! # What this phase can honestly assert, and what it cannot
//!
//! Golden cases 2, 3, and 4 are *product* claims: a page that explains a technology will not be
//! drawn, route cards per Grant Site, a completeness warning scoped to the evidence a fault
//! actually touched. None of that exists yet, and none of it is asserted here. Phase 6 owns it
//! (`docs/implementation-plan.md`, Phase 6 tasks 2–4).
//!
//! What exists is the seam below it, and the honest claim at that seam is narrow: the fixtures
//! parse, they resolve through the implemented rows, and where a row cannot yet answer they
//! reach a *named* open cell rather than a guess. Each case below states which of those it is
//! making. Two gaps are worth naming outright rather than leaving to be discovered:
//!
//! - **Drawability is not modeled.** A `factor = 0` clause is a container the technologies row
//!   preserved. That it *means* "will not appear through normal random research" is golden case
//!   2's product claim, and asserting it here would be asserting a projection nothing computes.
//! - **Grants are not modeled.** [`ReferenceKind`](super::ReferenceKind) is a closed four-variant
//!   set and none of its variants is a grant; nothing walks an event body looking for one, and no
//!   reverse index leads from a technology to the actions that award it. So golden case 3's two
//!   enclosing actions are asserted as *structures present in resolved event bodies*, not as
//!   routes derived from them.
//!
//! Stating those here is the point. A fixture committed with no expectation is indistinguishable
//! from a fixture nobody reads, and an expectation that overstated its seam would let Phase 6
//! inherit a green suite for work it has not done.

use crate::analysis::parser::{
    self, EvidenceQuality, Item, ParseFaultKind, ParsedFile, SourceIdentity, Value,
};
use crate::canonical::numeric::SourceNumber;
use crate::canonical::path::LogicalPath;
use crate::source::SourceKind;

use super::registry::{PolicyCell, Refusal};
use super::resolve;
use super::resolved::{ConstantOutcome, ResolvedDefinition};
use super::trial::{
    ENIGMALITH, ENIGMALITH_MEGASTRUCTURE_BODY, MALFORMED, MALFORMED_RECOVERY_BODY,
    MALFORMED_STRAY_BODY, ZERO_WEIGHT, corpus, enigmalith_vanilla, malformed_vanilla, named,
    zero_weight_vanilla,
};

// --- Golden case 4: malformed source ---

/// The definition a fault costs outright, and the evidence quality it costs everything after.
///
/// Asserted at the parser seam because that is the only seam that owns the answer:
/// `EvidenceQuality` and `ParseFault` are forbidden from the resolver's output by
/// `resolved::tests::resolved_output_names_no_parse`, so "this definition is Recovered" is not a
/// question a `ResolvedRegistry` can be asked. Golden case 4's "no failed input is silently
/// omitted" needs both halves — what disappeared, and what survived less trustworthy.
#[test]
fn golden_case_4_a_fault_costs_one_definition_and_downgrades_what_follows() {
    let file = parse_fixture(
        "common/technology/malformed_recovery.txt",
        MALFORMED_RECOVERY_BODY,
    );

    assert_eq!(
        faults(&file),
        [(
            ParseFaultKind::ContainerUnclosed,
            Some(line_start(
                MALFORMED_RECOVERY_BODY,
                b"tech_malformed_recovered"
            )),
        )],
        "one fault, resuming at the next column-zero identifier after it",
    );
    assert_eq!(
        definitions(&file),
        [
            ("tech_malformed_clean".to_owned(), EvidenceQuality::Clean),
            (
                "tech_malformed_recovered".to_owned(),
                EvidenceQuality::Recovered,
            ),
        ],
        "the unclosed definition is absent, and what follows it is Recovered rather than Clean",
    );
}

/// The other bound: a fault that costs no definition at all.
///
/// Its value is entirely in sitting beside the case above. A completeness warning derived from
/// "this file faulted" would treat these two files identically, and they are not identical —
/// one lost a technology and one lost only the claim that its nesting is trustworthy.
#[test]
fn golden_case_4_a_stray_brace_costs_evidence_quality_and_no_definition() {
    let file = parse_fixture(
        "common/technology/malformed_stray_brace.txt",
        MALFORMED_STRAY_BODY,
    );

    assert_eq!(
        faults(&file),
        [(
            ParseFaultKind::UnexpectedCloseBrace,
            Some(line_start(MALFORMED_STRAY_BODY, b"tech_stray_recovered")),
        )],
    );
    assert_eq!(
        definitions(&file),
        [
            ("tech_stray_clean".to_owned(), EvidenceQuality::Clean),
            (
                "tech_stray_recovered".to_owned(),
                EvidenceQuality::Recovered,
            ),
        ],
        "both definitions survive a stray close brace; only the second one's quality drops",
    );
}

/// Partial generation at the resolver seam: every key a fault did not cost resolves, and the one
/// it did cost is absent rather than guessed at.
#[test]
fn golden_case_4_the_corpus_resolves_every_definition_no_fault_cost() {
    let (vanilla, target) = (
        malformed_vanilla(),
        corpus(SourceKind::TargetMod, MALFORMED),
    );
    let resolution = resolve(&vanilla, &target);
    let registry = named(&resolution, "technologies");

    assert_eq!(
        registry.keys(),
        [
            "tech_intact_baseline",
            "tech_malformed_baseline",
            "tech_malformed_clean",
            "tech_malformed_recovered",
            "tech_stray_clean",
            "tech_stray_recovered",
        ],
        "two faulted files cost exactly one definition between them, and nothing else",
    );
    assert!(
        registry.get("tech_malformed_absent").is_none(),
        "a definition the parser could not emit must be absent, never reconstructed",
    );

    // The Vanilla side is the control that keeps the claim above from being about the corpus
    // rather than about the files: impact propagates along recorded dependency edges only
    // (`docs/technical-design.md`, "Analysis Issue impact"), and no edge leads here.
    let baseline = registry
        .get("tech_malformed_baseline")
        .expect("the uncontested Vanilla key resolves");
    assert_eq!(
        scalar(baseline, "cost"),
        Some("50".to_owned()),
        "a mod-side fault leaves an uncontested Vanilla definition exactly as stated",
    );
    assert_eq!(baseline.position.source, SourceKind::VanillaContent);
}

// --- Golden case 2: conditional zero-weight technology ---

/// The `×0` clause survives resolution, on the subject and on nothing else.
///
/// This asserts *preservation*, not drawability. What golden case 2 ultimately needs — a page
/// that says the technology will not appear through normal random research while the condition
/// holds, with the other modifiers still visible — is Phase 6's. What it needs from Phase 4 is
/// that the zero is still here to be read, distinguishable from a neighbouring modifier and from
/// the prerequisite `D-008` warns reads like the subject.
#[test]
fn golden_case_2_the_zero_factor_survives_resolution_on_the_subject_alone() {
    let (vanilla, target) = (
        zero_weight_vanilla(),
        corpus(SourceKind::TargetMod, ZERO_WEIGHT),
    );
    let resolution = resolve(&vanilla, &target);
    let registry = named(&resolution, "technologies");

    let subject = registry.get("tech_zero_weight_subject").expect("resolves");
    let control = registry.get("tech_zero_weight_control").expect("resolves");
    let prerequisite = registry
        .get("tech_zero_weight_prerequisite")
        .expect("resolves");
    let untouched = registry
        .get("tech_zero_weight_untouched")
        .expect("resolves");

    assert_eq!(
        modifier_factors(subject),
        ["2", "0.25", "0"],
        "the gated zero survives, and so do the positive and negative modifiers beside it",
    );
    assert_eq!(
        modifier_factors(control),
        ["2", "0.25", "0.5"],
        "the matched control differs from the subject in exactly this one factor",
    );
    assert_eq!(
        modifier_factors(prerequisite),
        ["2", "0.25"],
        "the decoy carries the shared modifiers and never the zero",
    );
    assert!(
        !untouched.states("weight_modifier"),
        "the uncontested Vanilla key has no modifiers at all",
    );

    // Base weight and the zero factor are different facts about different things, and golden
    // case 2 turns on their being different: the technology is drawable-weighted and multiplied
    // to nothing, not weighted at nothing.
    assert_eq!(scalar(subject, "weight"), Some("100".to_owned()));
    assert!(
        subject.states("potential"),
        "eligibility is stated, so 'otherwise eligible' is observable rather than vacuous",
    );
}

// --- Golden case 3: the Enigmalith shape ---

/// A zero base Draw Weight that is a *resolved constant*, not a literal zero.
///
/// The distinction is the reason golden case 3 asks for scripted constants at all. A row that
/// published `@enigmalith_zero_draw` as though it were a value would produce the same rendered
/// zero as one that resolved it, and `D-130` names that hazard directly. So both halves are
/// asserted: the effective field still holds the reference text, and the resolved value arrives
/// separately as a fact naming the declaration it came from.
#[test]
fn golden_case_3_the_zero_base_draw_weight_resolves_from_a_vanilla_constant() {
    let (vanilla, target) = (
        enigmalith_vanilla(),
        corpus(SourceKind::TargetMod, ENIGMALITH),
    );
    let resolution = resolve(&vanilla, &target);
    let registry = named(&resolution, "technologies");

    let subject = registry.get("tech_enigmalith_subject").expect("resolves");
    assert_eq!(
        scalar(subject, "weight"),
        Some("@enigmalith_zero_draw".to_owned()),
        "the effective field keeps the reference, so a Source Excerpt still cites real source",
    );
    assert_eq!(
        constant_value(subject, "weight"),
        SourceNumber::parse("0").value().cloned(),
        "and the resolved base value is exactly zero",
    );
    assert_eq!(
        constant_declaration(subject, "weight"),
        Some(SourceKind::VanillaContent),
        "resolved across sources: the mod reads a constant Vanilla declares",
    );

    // The matched control is what makes zero a result. Both keys read a Vanilla constant by the
    // same route, so neither "every symbol resolved to zero" nor "the wrong declaration won"
    // could survive this pair.
    let control = registry.get("tech_enigmalith_control").expect("resolves");
    assert_eq!(
        constant_value(control, "weight"),
        SourceNumber::parse("25").value().cloned(),
    );
    for definition in [subject, control] {
        assert_eq!(
            constant_value(definition, "cost"),
            SourceNumber::parse("4000").value().cloned(),
            "the resolved base cost golden case 3 also names",
        );
    }
}

/// Two distinct enclosing actions granting one technology.
///
/// The claim is deliberately about *structure*, because that is all this seam has: the events row
/// resolves registration, and these bodies are retained exactly as parsed. So the assertion is
/// that the corpus really does hold two separate actions naming the same technology — one an
/// `immediate`, one an `option` — which is the evidence `D-051`'s "shared terminal effects do not
/// merge their preceding routes" will be tested against once Grant Sites exist.
#[test]
fn golden_case_3_two_distinct_enclosing_actions_grant_the_same_technology() {
    let (vanilla, target) = (
        enigmalith_vanilla(),
        corpus(SourceKind::TargetMod, ENIGMALITH),
    );
    let resolution = resolve(&vanilla, &target);
    let registry = named(&resolution, "events");

    assert_eq!(
        registry.keys(),
        [
            "enigmalith.databank",
            "enigmalith.final_spark",
            "enigmalith.notice"
        ],
    );

    let subject = "tech_enigmalith_subject";
    assert_eq!(
        granting_fields(
            registry.get("enigmalith.databank").expect("resolves"),
            subject
        ),
        ["immediate"],
    );
    assert_eq!(
        granting_fields(
            registry.get("enigmalith.final_spark").expect("resolves"),
            subject,
        ),
        ["option"],
        "a different enclosing action from the Databank route's, not a second copy of it",
    );
    assert!(
        granting_fields(
            registry.get("enigmalith.notice").expect("resolves"),
            subject
        )
        .is_empty(),
        "the scoping control: not every event in this corpus names the technology",
    );
}

/// The megastructure entry exists, and the row that would interpret it refuses visibly.
///
/// Both halves, because neither is evidence alone. The refusal would be just as green over a
/// corpus containing no megastructure at all — it is raised from the row's eager field cell
/// before a file is read — so the parser seam is what shows the content is really there. And the
/// content alone would say nothing about whether the resolver guesses at it.
///
/// Closing the cell is STE-22's stretch capture. When it closes, this is already the corpus.
#[test]
fn golden_case_3_the_megastructure_is_present_and_its_row_refuses_on_a_named_cell() {
    let file = parse_fixture(
        "common/megastructures/zz_enigmalith_megastructures.txt",
        ENIGMALITH_MEGASTRUCTURE_BODY,
    );
    assert!(file.faults.is_empty(), "{:?}", file.faults);
    assert_eq!(
        definitions(&file),
        [(
            "megastructure_enigmalith_site".to_owned(),
            EvidenceQuality::Clean,
        )],
        "the entry golden case 3 needs is in the corpus, parsed cleanly",
    );

    let (vanilla, target) = (
        enigmalith_vanilla(),
        corpus(SourceKind::TargetMod, ENIGMALITH),
    );
    let resolution = resolve(&vanilla, &target);

    let refusal = resolution
        .registry("megastructures")
        .expect_err("the megastructures row's field cell is open");
    assert!(
        matches!(
            refusal,
            Refusal::UnresolvedCell {
                registry: "megastructures",
                cell: PolicyCell::FieldRule,
                ..
            }
        ),
        "an unresolved cell must fail visibly, by name: {refusal}",
    );
}

// --- Helpers ---

fn parse_fixture(logical: &str, bytes: &[u8]) -> ParsedFile {
    let identity = SourceIdentity::new(
        SourceKind::TargetMod,
        LogicalPath::parse(logical).expect("a literal logical path"),
    );
    parser::parse(identity, bytes)
}

fn faults(file: &ParsedFile) -> Vec<(ParseFaultKind, Option<u64>)> {
    file.faults
        .iter()
        .map(|fault| (fault.kind, fault.recovery_boundary))
        .collect()
}

fn definitions(file: &ParsedFile) -> Vec<(String, EvidenceQuality)> {
    file.definitions()
        .map(|(field, evidence)| (field.key.text().into_owned(), evidence))
        .collect()
}

/// The byte offset of the column-zero line `needle` begins, so a recovery boundary is asserted
/// against the fixture's own layout rather than against a number that silently means nothing
/// after an edit.
///
/// Column zero is load-bearing, not decoration: every fixture here names its own keys in a header
/// comment describing the control discipline, and a match inside that prose would resolve to a
/// byte offset in the comment — which is exactly the wrong answer, silently. The same requirement
/// is why `jomini::tests::find_line_start` exists.
fn line_start(haystack: &[u8], needle: &[u8]) -> u64 {
    haystack
        .windows(needle.len())
        .enumerate()
        .find_map(|(index, window)| {
            (window == needle && (index == 0 || haystack[index - 1] == b'\n'))
                .then_some(index as u64)
        })
        .unwrap_or_else(|| {
            panic!(
                "{} begins no line in the fixture",
                String::from_utf8_lossy(needle)
            )
        })
}

/// One effective field's value as source text, for the scalar fields these cases read.
fn scalar(definition: &ResolvedDefinition, field: &str) -> Option<String> {
    match definition.field(field)? {
        Value::Scalar(scalar) => Some(scalar.text().into_owned()),
        Value::Container(_) | Value::Tagged { .. } => None,
    }
}

/// Every `factor` inside a definition's `weight_modifier`, in source order.
fn modifier_factors(definition: &ResolvedDefinition) -> Vec<String> {
    let Some(Value::Container(container)) = definition.field("weight_modifier") else {
        return Vec::new();
    };
    container
        .fields()
        .filter(|field| field.key.text() == "modifier")
        .filter_map(|field| match &field.value {
            Value::Container(modifier) => Some(modifier),
            Value::Scalar(_) | Value::Tagged { .. } => None,
        })
        .flat_map(|modifier| {
            modifier
                .fields()
                .filter(|field| field.key.text() == "factor")
                .filter_map(|field| match &field.value {
                    Value::Scalar(scalar) => Some(scalar.text().into_owned()),
                    Value::Container(_) | Value::Tagged { .. } => None,
                })
        })
        .collect()
}

fn constant_fact<'a>(
    definition: &'a ResolvedDefinition,
    field: &str,
) -> &'a super::resolved::ConstantFact {
    definition
        .constants
        .iter()
        .find(|fact| fact.field.as_deref() == Some(field))
        .unwrap_or_else(|| {
            panic!(
                "{field} carries a constant fact: {:?}",
                definition.constants
            )
        })
}

fn constant_value(
    definition: &ResolvedDefinition,
    field: &str,
) -> Option<crate::canonical::numeric::ExactValue> {
    match &constant_fact(definition, field).outcome {
        ConstantOutcome::Resolved { value, .. } => value.value().cloned(),
        ConstantOutcome::Unresolved(unresolved) => {
            panic!("{field} was expected to resolve: {unresolved:?}")
        }
    }
}

fn constant_declaration(definition: &ResolvedDefinition, field: &str) -> Option<SourceKind> {
    match &constant_fact(definition, field).outcome {
        ConstantOutcome::Resolved { declaration, .. } => declaration.source(),
        ConstantOutcome::Unresolved(unresolved) => {
            panic!("{field} was expected to resolve: {unresolved:?}")
        }
    }
}

/// The top-level fields of a definition whose subtree names `needle` — for an event body, the
/// enclosing actions a grant appears inside.
///
/// Top-level because that is the unit golden case 3 separates: `immediate` and `option` are the
/// two enclosing actions, and how deeply the grant sits inside either is not what distinguishes
/// them.
fn granting_fields(definition: &ResolvedDefinition, needle: &str) -> Vec<String> {
    definition
        .fields
        .iter()
        .filter(|field| names(&field.value, needle))
        .map(|field| field.field.clone())
        .collect()
}

fn names(value: &Value, needle: &str) -> bool {
    match value {
        Value::Scalar(scalar) => scalar.text() == needle,
        Value::Container(container) => container.items.iter().any(|item| item_names(item, needle)),
        Value::Tagged { tag, container, .. } => {
            tag.text() == needle || container.items.iter().any(|item| item_names(item, needle))
        }
    }
}

fn item_names(item: &Item, needle: &str) -> bool {
    match item {
        Item::Field(field) => field.key.text() == needle || names(&field.value, needle),
        Item::Element(value) => names(value, needle),
        Item::Conditional(conditional) => conditional
            .items
            .iter()
            .any(|item| item_names(item, needle)),
    }
}
