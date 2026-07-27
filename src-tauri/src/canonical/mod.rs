//! Shared canonical primitives used by every stable identity: domain-separated digests
//! over a tagged length-prefixed encoding, logical relative paths, and exact numerics
//! (docs/technical-design.md, "Canonicalization and numeric representation").
//!
//! Not part of the technical design's named module map: this is a leaf primitive module
//! below the deep-module row. Each identity's field order and schema remain owned by the
//! module that defines that identity; only the encoding mechanics are shared.

pub mod encode;
pub mod path;
