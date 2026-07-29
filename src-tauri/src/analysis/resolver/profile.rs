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

use super::registry::{
    CellStatus, CrossSourceRule, FieldRule, KeyRule, OrderingRule, ProvenanceRule,
    ReferenceHandling, ReferenceRule, RegistryPolicy, RepeatRule, Replacement, ShadowUnit,
    StreamScope, top_level_definitions,
};
use super::resolved::{FactKind, ReferenceKind};
use super::stream::{ContentFamily, FileScope};

/// The `resolution_profile` component of the analysis version vector.
///
/// - 1: the Phase 4D core — common file selection, per-family semantic streams, the repeat
///   rule, provenance, and visible refusal. No registry row is declared yet.
/// - 2: the technologies row, and a per-kind references cell the engine honours by detecting
///   scripted-constant and inline-script references without resolving them.
pub(in crate::analysis) const RESOLUTION_PROFILE_VERSION: u32 = 2;

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
///   rules. Technologies declare no inner identifier the way ship components do.
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
/// - **References.** Detected, not resolved. Technology bodies carry `@constant` references
///   and `inline_script` inclusions, and both belong to rows with their own evidence and
///   their own tickets. Declaring them here is what makes their presence *visible* in the
///   meantime: an effective `cost` of `@tier5cost3` carries a fact saying so instead of
///   reading as a literal value.
/// - **Provenance.** The four kinds the matrix names. `Inherited` is deliberately absent:
///   whole-object replacement never produces one, and an undeclared kind is a refusal, so
///   this is the field rule's claim stated where the engine can catch it breaking.
const TECHNOLOGIES: RegistryPolicy = RegistryPolicy {
    name: "technologies",
    key: CellStatus::Resolved(KeyRule {
        reader: top_level_definitions,
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
                ReferenceHandling::DetectedNotResolved,
            ),
            (
                ReferenceKind::InlineScript,
                ReferenceHandling::DetectedNotResolved,
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

/// The registry rows this profile declares.
///
/// One row per ticket, because each is a unit of evidence that deserves its own review
/// (`docs/implementation-plan.md`, "Ticketing", granularity 3). Until a row is declared here,
/// asking for it is [`Refusal::UndeclaredRegistry`](super::registry::Refusal) — which is the
/// design's "a content type may be claimed as supported only when every policy it requires is
/// explicit and oracle-backed", enforced rather than intended.
pub(super) const DECLARED: &[RegistryPolicy] = &[TECHNOLOGIES];

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
    use super::super::resolved::{ReferenceFact, ReferenceKind};

    /// One Target Mod technology, resolved through the declared row against an empty base.
    fn only_technology(body: &str) -> SourceSnapshot {
        FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"references\"")
            .with_file("common/technology/zz_references.txt", body.as_bytes())
            .build()
            .expect("a well-formed fixture corpus")
    }

    fn references(body: &str) -> Vec<ReferenceFact> {
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file("common/technology/00_empty.txt", b"")
            .build()
            .expect("a well-formed fixture corpus");
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

    /// The scripted-constant half of the technologies row's references cell.
    ///
    /// A pin on *deferred* behaviour, not on desired behaviour. `r1` proved the game resolves
    /// a vanilla `@` constant read from a mod file, so the effective `cost` here is wrong as a
    /// value — and the row's answer is to say so rather than to publish `@tier5cost3` as
    /// though it were a number.
    ///
    /// **This test is scheduled to go red in STE-27 (Phase 4F).** When that ticket teaches the
    /// engine to resolve scripted constants, this row's `ScriptedConstant` entry changes and
    /// the assertion below becomes the wrong expectation — which is the point: the flip has to
    /// be a decision somebody makes, not a behaviour that quietly arrives.
    #[test]
    fn a_scripted_constant_reference_is_detected_and_left_unresolved() {
        let found = references("tech_references = {\n\tcost = @tier5cost3\n\ttier = 1\n}\n");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, ReferenceKind::ScriptedConstant);
        assert_eq!(found[0].field, "cost");

        // And the value is still the reference text, not a resolved number and not a hole.
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file("common/technology/00_empty.txt", b"")
            .build()
            .expect("a well-formed fixture corpus");
        let target = only_technology("tech_references = {\n\tcost = @tier5cost3\n\ttier = 1\n}\n");
        let registry = resolve(&vanilla, &target)
            .registry("technologies")
            .expect("the declared row resolves");
        let definition = registry.get("tech_references").expect("the key resolves");
        assert!(definition.states("cost"));
        assert!(
            definition.states("tier"),
            "the literal fields are unaffected"
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
