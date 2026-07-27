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
            source_enumeration: 1,
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
        assert_eq!(
            AnalysisVersionVector::current().digest().to_hex(),
            "49cfc070bfea607cf7b100761587ebe9599e0ce106e963736ae3882506c98641"
        );
    }

    #[test]
    fn every_component_and_recipe_participates_in_the_digest() {
        let base = AnalysisVersionVector::current();
        let mut bumped = base.clone();
        bumped.parsed_model += 1;
        assert_ne!(base.digest(), bumped.digest());

        let mut with_recipe = base.clone();
        with_recipe.asset_recipes.insert("dds-to-png".to_owned(), 1);
        assert_ne!(base.digest(), with_recipe.digest());
    }
}
