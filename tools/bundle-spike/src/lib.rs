//! Evidence harness for [the revision bundle evaluation](../../../docs/spikes/revision-bundle-evaluation.md).
//!
//! `docs/technical-design.md` was written assuming that a Documentation Revision is a
//! directory of build-time denormalized JSON, and pre-authorizing a content-addressed
//! Localization Store as the first fallback if preserved localization turns out to dominate
//! cross-revision duplication. Neither assumption has been measured. This spike measures
//! them, against budgets declared before the deciding numbers were collected.
//!
//! The measurement cannot be taken over raw Mod Source sizes. What a bundle costs is a fact
//! about *generated documentation*, which is several transformations away from the bytes on
//! disk: resolution collapses duplicate definitions, generation expands one technology into
//! browse summary, search material, and full record, and localization is preserved for every
//! available language rather than one. So the harness has to build the thing before it can
//! weigh it.
//!
//! Two structural rules follow from the other spikes in this repository and are load-bearing
//! here:
//!
//! - **One canonical model, every view derived.** [`docmodel`] holds the documentation model
//!   once; [`bundle`] writes every shape from that one value. If two writers implemented the
//!   same rule independently, a size comparison between them would be measuring the
//!   divergence of two generators rather than the cost of a format.
//! - **Identity and timing are recorded separately.** [`record`] drift-compares corpus
//!   digests, versions, content hashes, sizes, and counts byte for byte, and never compares
//!   a wall-clock number. `d3-recipe` found the alternative in itself: a timing figure in a
//!   drift-compared record makes every re-capture differ for a reason that has nothing to do
//!   with the evidence.
//!
//! This crate is throwaway. If the spike concludes in favour of materialized JSON, it informs
//! `revisions`, `search`, and `assets` rather than becoming them.

pub mod assets;
pub mod bundle;
pub mod corpus;
pub mod digest;
pub mod docmodel;
pub mod generate;
pub mod localization;
pub mod locstore;
pub mod pipeline;
pub mod reader;
pub mod record;
pub mod resolve;
pub mod search;
pub mod timing;
