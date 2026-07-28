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

use super::registry::RegistryPolicy;

/// The `resolution_profile` component of the analysis version vector.
///
/// - 1: the Phase 4D core — common file selection, per-family semantic streams, the repeat
///   rule, provenance, and visible refusal. No registry row is declared yet.
pub(in crate::analysis) const RESOLUTION_PROFILE_VERSION: u32 = 1;

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

/// The registry rows this profile declares.
///
/// Deliberately empty. The Phase 4D core is the machinery a row instantiates; each row is
/// its own ticket because each is a unit of evidence that deserves its own review
/// (`docs/implementation-plan.md`, "Ticketing", granularity 3). Until a row is declared here,
/// asking for it is [`Refusal::UndeclaredRegistry`](super::registry::Refusal) — which is the
/// design's "a content type may be claimed as supported only when every policy it requires is
/// explicit and oracle-backed", enforced rather than intended.
pub(super) const DECLARED: &[RegistryPolicy] = &[];

pub(super) fn declared(name: &str) -> Option<&'static RegistryPolicy> {
    DECLARED.iter().find(|policy| policy.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::version::AnalysisVersionVector;

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
