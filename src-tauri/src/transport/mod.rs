//! Input adapters: Tauri commands and Companion HTTP requests adapted into shared
//! application DTOs and failure semantics. Adapters call application use cases; they do
//! not implement parallel rules and do not call through one another.

pub mod http;
pub mod tauri;
