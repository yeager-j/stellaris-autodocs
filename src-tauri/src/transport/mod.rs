//! Input adapters: Tauri commands and Companion HTTP requests adapted into shared
//! application DTOs and failure semantics. Adapters call application use cases; they do
//! not implement parallel rules and do not call through one another.

//! **Wire naming lives here and nowhere else.** Transport DTOs carry
//! `#[serde(rename_all = "camelCase")]` because they are read by TypeScript. Persisted formats
//! — revision bundles, the state document — stay snake_case and must not inherit it: they are
//! read by this crate across versions, and a rename there is a schema change.

pub mod envelope;
pub mod http;
pub mod tauri;

pub use envelope::{Envelope, Rejection, resolve};

#[cfg(test)]
mod contract_vectors;
