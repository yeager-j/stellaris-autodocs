//! Sole owner of Documentation Revision bundle I/O: staging, validation, atomic
//! publication, the Revision Reader, handle pinning, and retention
//! (docs/technical-design.md, "Documentation revision publication"). Populated in Phase 3.

pub mod candidate;
pub mod manifest;
