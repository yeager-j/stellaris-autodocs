//! Deep owner of the durable mutable state document: schema, atomic replacement,
//! quarantine recovery, and the narrow publication-reference capability
//! (docs/technical-design.md, "Mutable state storage"). Populated in Phase 1.

pub mod model;
mod mutations;
pub mod replace;
pub mod store;

pub use model::{AppState, CURRENT_SCHEMA, DiscoveryLocation, PublicationReference, RevisionId};
pub use mutations::{MutationCommit, MutationError, PublicationCapability, PublicationError};
pub use store::{OpenOutcome, OpenReport, STATE_FILE, StateStore};
