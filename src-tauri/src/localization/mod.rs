//! Owns the Stellaris localization language: ingestion, markup tokenization, fallback,
//! Static Localization Reference resolution, plain-text projection, and display tokens
//! (docs/technical-design.md, "Localization module"). Populated in Phase 5.

pub mod detect;
pub mod effective;
pub mod language;

pub use detect::{DetectedGameLanguage, detect_language, language_from_settings};
pub use effective::{EffectiveLanguage, LanguageSource, derive_effective_language};
pub use language::{LanguageTag, LanguageTagError, language_override_from_document};
