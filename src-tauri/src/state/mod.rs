//! Deep owner of the durable mutable state document: schema, atomic replacement,
//! quarantine recovery, and the narrow publication-reference capability
//! (docs/technical-design.md, "Mutable state storage"). Populated in Phase 1.

pub mod model;
pub mod replace;
