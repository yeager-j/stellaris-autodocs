//! Finds Stellaris and Mod Installations and reads only the metadata needed to populate
//! the Mod Library. Never a second fingerprint implementation
//! (docs/technical-design.md, "Source module"). Populated in Phase 1.

mod descriptor;
pub mod identity;

pub use descriptor::DescriptorMetadata;
