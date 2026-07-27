//! Application library. The executable target only calls [`run`]; everything else is
//! constructed by the composition root (docs/technical-design.md, "Rust package and
//! dependency direction"). Dependency direction: transports -> application -> deep
//! modules -> filesystem, parser, image-decoder, and persistence adapters.

pub mod analysis;
pub mod application;
pub mod assets;
pub mod canonical;
pub mod companion;
pub mod composition;
pub mod discovery;
pub mod error;
pub mod localization;
pub mod revisions;
pub mod search;
pub mod source;
pub mod state;
pub mod transport;

#[cfg(feature = "test-support")]
pub mod testsupport;

pub use composition::run;
