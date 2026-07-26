//! Evidence harness for [the parser evaluation](../../../docs/spikes/parser-evaluation.md).
//!
//! The spike asks whether Jomini can back the application's parser adapter. It answers that
//! by building two adapters over one application-owned model and using the mature one to
//! check the one that adds what the design needs:
//!
//! - [`tape`] — Path A. `TextTape`, no structural source ranges, whole-file failure.
//! - [`lexer`] — Path B. `TokenReader` plus `position()`, byte ranges on every node, and
//!   resynchronization past a syntax fault.
//!
//! Path A is Jomini's fuzz-tested front end, so on every file both parse, Path B's model
//! must be structurally identical to Path A's modulo spans. The count and shape of the
//! divergences over the real corpus is the measurement that decides whether a hand-built
//! wrapper is maintainable or is quietly a second parser.
//!
//! This crate is throwaway. If the spike concludes in favour of adoption, it informs
//! `analysis::parser` rather than becoming it.

pub mod checks;
pub mod classify;
pub mod corpus;
pub mod digest;
pub mod lexer;
pub mod model;
pub mod record;
pub mod survey;
pub mod tape;
