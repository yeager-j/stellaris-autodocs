//! Error conventions shared by every module.
//!
//! Expected outcomes cross module and transport boundaries as typed `Result<T, E>` where
//! `E` is an operation-specific union (docs/technical-design.md, "Serializable result
//! contract"). Unexpected failures — invariant violations, corrupted cross-module
//! contracts, programmer defects — travel as [`Unexpected`], carrying a correlation
//! identifier that transports may show while detailed chains stay in protected desktop
//! logs.
//!
//! Panic policy: a panic is a defect, never control flow. Nothing intentionally panics
//! across a module boundary, and no panic is serialized to a transport. Transport
//! entrypoints catch and redact unexpected failures where the runtime can safely unwind.

use crate::canonical::hex;
use std::fmt;

/// Opaque identifier correlating a user-visible failure with protected log detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CorrelationId([u8; 16]);

impl CorrelationId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().into_bytes())
    }
}

impl fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        hex::write(f, &self.0)
    }
}

/// An unexpected internal failure: not a member of any operation's expected-error union.
#[derive(Debug)]
pub struct Unexpected {
    correlation: CorrelationId,
    message: String,
}

impl Unexpected {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            correlation: CorrelationId::generate(),
            message: message.into(),
        }
    }

    pub fn correlation(&self) -> CorrelationId {
        self.correlation
    }

    /// Full detail for protected desktop logs only. Never crosses a transport.
    pub fn log_detail(&self) -> String {
        format!("[{}] {}", self.correlation, self.message)
    }
}

impl fmt::Display for Unexpected {
    /// Redacted rendering, safe for transports: correlation identifier only.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unexpected internal error [{}]", self.correlation)
    }
}

impl std::error::Error for Unexpected {}

/// The failure channel of an application operation: an expected, typed refusal or an
/// unexpected internal error.
#[derive(Debug)]
pub enum Failure<E> {
    Expected(E),
    Unexpected(Unexpected),
}

impl<E> From<Unexpected> for Failure<E> {
    fn from(unexpected: Unexpected) -> Self {
        Self::Unexpected(unexpected)
    }
}

pub type OpResult<T, E> = Result<T, Failure<E>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlation_ids_are_32_hex_characters_and_unique() {
        let first = CorrelationId::generate();
        let second = CorrelationId::generate();
        let rendered = first.to_string();
        assert_eq!(rendered.len(), 32);
        assert!(rendered.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }

    #[test]
    fn unexpected_display_redacts_detail_but_carries_the_correlation_id() {
        let failure = Unexpected::new("state file vanished mid-mutation: /Users/x/secret");
        let shown = failure.to_string();
        assert!(shown.contains(&failure.correlation().to_string()));
        assert!(!shown.contains("secret"));
        assert!(failure.log_detail().contains("secret"));
    }

    #[test]
    fn unexpected_converts_into_any_failure_union() {
        #[derive(Debug)]
        enum SomeExpected {}
        let failure: Failure<SomeExpected> = Unexpected::new("boom").into();
        assert!(matches!(failure, Failure::Unexpected(_)));
    }
}
