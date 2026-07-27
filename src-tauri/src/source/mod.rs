//! Sole owner of complete Mod Source traversal and content identity: deterministic
//! enumeration, logical-path normalization and escape rejection, hashing, fingerprints,
//! build-lifetime Source Snapshots, and final live-source verification
//! (docs/technical-design.md, "Source module"). Populated in Phase 2.

pub mod enumerate;
pub mod policy;
