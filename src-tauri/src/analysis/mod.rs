//! The deep module that turns Source Snapshots plus typed asset-materialization outcomes
//! into a finalized Revision Candidate. Parser adaptation, content-type-specific
//! resolution, documentation generation, and Source Excerpt capture remain internal
//! submodules (docs/technical-design.md, "Analysis module"). Populated from Phase 4.

#[cfg(test)]
mod conformance;
#[cfg(test)]
mod corpora;
/// The build stage that hands resolved localization bytes to `localization`. Dead until the
/// Phase 6 driver calls it, for the same reason [`resolver`] is.
#[allow(dead_code)]
mod localization_stage;
#[allow(dead_code)]
mod parser;
#[allow(dead_code)]
mod resolver;
pub mod version;
