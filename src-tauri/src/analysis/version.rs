//! The analysis version vector: every semantic component whose change invalidates
//! previously built revisions (docs/technical-design.md, "Canonicalization and numeric
//! representation"). Each component starts at 1 and is bumped by hand in the phase that
//! changes the component's behavior.

use crate::canonical::encode::{CanonicalDigest, DigestBytes, ENCODING_VERSION};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub asset_recipes: BTreeMap<String, u32>,
}

impl AnalysisVersionVector {
    pub fn current() -> Self {
        Self {
            // 2: the source fingerprint moved to domain /v2, framing each entry as a
            // nested two-item sequence. The scheme that names a build's inputs changed,
            // so the component that invalidates builds changes with it — the protocol
            // stated in source::fingerprint. No revision has been published yet, so the
            // bump invalidates nothing today; following it now is what keeps the rule
            // from being learned as optional.
            source_enumeration: 2,
            parsed_model: 1,
            resolution_profile: 1,
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
    fn pinned_current_digest() {
        // Pinned regression value: any component bump changes this and must re-pin it,
        // which is exactly the review moment the version vector exists to force.
        // Re-derived independently for source_enumeration = 2 (Phase 2A, fingerprint
        // domain /v2).
        assert_eq!(
            AnalysisVersionVector::current().digest().to_hex(),
            "aecc912e9f7874b5617b77260dc8189540a8db8a05e8c6b9b0238989acf5bbe6"
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
