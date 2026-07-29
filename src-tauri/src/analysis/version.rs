//! The analysis version vector: every semantic component whose change invalidates
//! previously built revisions (docs/technical-design.md, "Canonicalization and numeric
//! representation"). Each component starts at 1 and is bumped by hand in the phase that
//! changes the component's behavior.

use super::resolver::RESOLUTION_PROFILE_VERSION;
use crate::canonical::encode::{CanonicalDigest, DigestBytes, ENCODING_VERSION};
use crate::source::policy::ENUMERATION_POLICY_VERSION;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Serde here is presentation only: a revision manifest records the vector it was built
/// under so a reader can see which component changed. **Identity is [`digest()`], never
/// this JSON form** — field order, key spelling, and map ordering are the serializer's
/// business, and hashing them would make the identity depend on a library's choices
/// rather than on `canonical::encode`.
///
/// [`digest()`]: AnalysisVersionVector::digest
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisVersionVector {
    pub source_enumeration: u32,
    pub parsed_model: u32,
    pub resolution_profile: u32,
    /// Documentation schema and generator together.
    pub documentation: u32,
    pub localization_interpretation: u32,
    /// Search normalization and index schema together.
    pub search: u32,
    pub canonical_encoding: u32,
    pub hidden_route_identity: u32,
    pub analysis_issue_propagation: u32,
    /// One entry per asset conversion recipe, keyed by recipe identifier.
    /// Empty until Phase 8 registers the DDS-to-PNG recipe.
    #[serde(default)]
    pub asset_recipes: BTreeMap<String, u32>,
}

impl AnalysisVersionVector {
    pub fn current() -> Self {
        Self {
            // Read from `source::policy`, which owns both the enumeration allowlists and
            // the fingerprint domain built over them. A literal here could not be made to
            // move when the policy moved; the constant can, and `pinned_policy_surface`
            // asserts it beside the allowlists it versions.
            source_enumeration: ENUMERATION_POLICY_VERSION,
            parsed_model: 1,
            // Read from `resolver::profile`, which owns the rows, the streams, and the game
            // build they were established against — the same reason `source_enumeration`
            // above quotes its policy's constant rather than repeating its value.
            resolution_profile: RESOLUTION_PROFILE_VERSION,
            documentation: 1,
            localization_interpretation: 1,
            search: 1,
            canonical_encoding: ENCODING_VERSION,
            hidden_route_identity: 1,
            analysis_issue_propagation: 1,
            asset_recipes: BTreeMap::new(),
        }
    }

    pub fn digest(&self) -> DigestBytes {
        let mut digest = CanonicalDigest::new("stellaris-docs/analysis-version-vector/v1");
        digest
            .u64(self.source_enumeration.into())
            .u64(self.parsed_model.into())
            .u64(self.resolution_profile.into())
            .u64(self.documentation.into())
            .u64(self.localization_interpretation.into())
            .u64(self.search.into())
            .u64(self.canonical_encoding.into())
            .u64(self.hidden_route_identity.into())
            .u64(self.analysis_issue_propagation.into())
            .begin_map(self.asset_recipes.len());
        for (recipe, version) in &self.asset_recipes {
            digest.text(recipe).u64((*version).into());
        }
        digest.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::encode::ENCODING_VERSION;

    #[test]
    fn current_tracks_the_canonical_encoding_version() {
        assert_eq!(
            AnalysisVersionVector::current().canonical_encoding,
            ENCODING_VERSION
        );
    }

    #[test]
    fn current_reads_the_enumeration_policy_version() {
        // The Phase 2B fork resolution: `source::policy` owns the version of the policy,
        // and this vector quotes it. Two literals could drift apart in the commit that
        // changed the policy; one constant cannot.
        assert_eq!(
            AnalysisVersionVector::current().source_enumeration,
            ENUMERATION_POLICY_VERSION
        );
    }

    #[test]
    fn decodes_with_every_optional_field_absent() {
        // A manifest written before Phase 8 registers any asset recipe carries no
        // `asset_recipes` key at all. Without a read-side default the whole manifest
        // becomes undecodable, which is the failure the no-`skip_serializing_if` rule
        // exists to prevent (docs/technical-design.md:651; precedent state/model.rs:67).
        let minimal: AnalysisVersionVector = serde_json::from_str(
            r#"{"source_enumeration":3,"parsed_model":1,"resolution_profile":3,
                "documentation":1,"localization_interpretation":1,"search":1,
                "canonical_encoding":1,"hidden_route_identity":1,
                "analysis_issue_propagation":1}"#,
        )
        .unwrap();
        assert_eq!(minimal, AnalysisVersionVector::current());
    }

    #[test]
    fn a_populated_vector_round_trips_and_keeps_its_digest() {
        // The vector is stored so a reader can name the component that invalidated a
        // revision; a decoded copy must therefore still identify the same analysis. The
        // digest assertion is what proves serde is presentation rather than identity —
        // it survives the round trip because it never depended on it.
        let mut vector = AnalysisVersionVector::current();
        vector.asset_recipes.insert("dds-to-png".to_owned(), 2);
        let encoded = serde_json::to_vec(&vector).unwrap();
        let decoded: AnalysisVersionVector = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, vector);
        assert_eq!(decoded.digest(), vector.digest());
    }

    #[test]
    fn pinned_current_digest() {
        // Pinned regression value: any component bump changes this and must re-pin it,
        // which is exactly the review moment the version vector exists to force.
        // Re-pinned for resolution_profile = 3 (Phase 4F, STE-27 — scripted-triggers,
        // scripted-effects, and scripted-constants rows; the technologies row's
        // `ScriptedConstant` entry resolves against the constants environment; the
        // reference cell becomes per-kind `CellStatus`). Every prior pin stays reachable
        // through this file's history; what matters is that the bump and the re-pin land
        // together.
        assert_eq!(
            AnalysisVersionVector::current().digest().to_hex(),
            "8770c756fe6b49c860ab888799c84f5a0ecede31156d315afd97f6fbf568859d"
        );
    }

    #[test]
    fn every_component_and_recipe_participates_in_the_digest() {
        type Bump = fn(&mut AnalysisVersionVector);
        let base = AnalysisVersionVector::current();
        let bumps: [(&str, Bump); 9] = [
            ("source_enumeration", |v| v.source_enumeration += 1),
            ("parsed_model", |v| v.parsed_model += 1),
            ("resolution_profile", |v| v.resolution_profile += 1),
            ("documentation", |v| v.documentation += 1),
            ("localization_interpretation", |v| {
                v.localization_interpretation += 1
            }),
            ("search", |v| v.search += 1),
            ("canonical_encoding", |v| v.canonical_encoding += 1),
            ("hidden_route_identity", |v| v.hidden_route_identity += 1),
            ("analysis_issue_propagation", |v| {
                v.analysis_issue_propagation += 1
            }),
        ];
        for (component, bump) in bumps {
            let mut bumped = base.clone();
            bump(&mut bumped);
            assert_ne!(base.digest(), bumped.digest(), "component {component}");
        }

        let mut with_recipe = base.clone();
        with_recipe.asset_recipes.insert("dds-to-png".to_owned(), 1);
        assert_ne!(base.digest(), with_recipe.digest());
    }
}
