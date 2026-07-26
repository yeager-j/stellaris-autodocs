//! Evidence harness for `docs/spikes/dds-evaluation.md`.
//!
//! The result it replaces established that two DXT5 technology icons converted to PNG and looked
//! right. That is feasibility. What the technical design needs is a contract: one typed outcome
//! per input, a conversion recipe whose parameters are pinned because their alternatives were
//! measured, and a correctness claim that holds over a corpus nobody can inspect by eye
//! (`docs/technical-design.md:497`).
//!
//! The method is the parser spike's: two independent readings of the same bytes, where every
//! disagreement is either a defect or a finding. Here the readings are `decode_a` (image_dds)
//! and `decode_b` (mask reinterpretation for the uncompressed classes, texture2ddecoder for the
//! compressed ones). A single decoder would have nothing to be wrong against — a channel swap
//! or a dropped alpha mask produces a plausible image, and plausible is exactly what visual
//! inspection cannot reject.

pub mod archives;
pub mod classify;
pub mod corpus;
pub mod decode_a;
pub mod decode_b;
pub mod digest;
pub mod encode;
pub mod fixtures;
pub mod header;
pub mod model;
pub mod recipe;
pub mod record;
pub mod references;
