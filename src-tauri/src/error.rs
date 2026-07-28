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
///
/// **This type must never become serializable.** Its `Debug` renders `message`, so a
/// `Serialize` impl — derived or hand-written — would put source content, absolute paths, and
/// whatever else a caller passed to [`Unexpected::new`] one `?` away from a transport. What
/// crosses a wire instead is
/// [`transport::envelope::Rejection`](crate::transport::envelope::Rejection), which has no
/// field that can hold text. Orphan rules mean only this crate can add the impl, so the
/// prohibition is enforceable here; `no_source_file_makes_unexpected_serializable` is the gate,
/// and it is a text scan rather than a proof.
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

    /// Drops whole-line comments, so prose *about* the prohibition — including this module's
    /// own — is not mistaken for a violation of it. Attribute lines survive, which is what the
    /// derive check below reads. Line-count is preserved so nothing above an item drifts.
    fn code_only(source: &str) -> String {
        source
            .lines()
            .map(|line| {
                if line.trim_start().starts_with("//") {
                    ""
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Every `.rs` file under this crate's `src`, so the scan cannot be defeated by adding
    /// the impl in a module the test forgot to name.
    fn crate_sources() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, found: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir)
                .expect("read a source directory")
                .flatten()
            {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, found);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    let source = std::fs::read_to_string(&path).expect("read a source file");
                    found.push((path.display().to_string(), code_only(&source)));
                }
            }
        }
        let mut found = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut found,
        );
        assert!(!found.is_empty(), "the scan found no sources to read");
        found
    }

    /// Returns whether `source` derives `Serialize` on the item declared by `declaration`,
    /// by reading the attributes immediately above it.
    fn derives_serialize(source: &str, declaration: &str) -> bool {
        let item = source
            .find(declaration)
            .unwrap_or_else(|| panic!("{declaration} is declared in this file"));
        source[..item]
            .rsplit("\n\n")
            .next()
            .expect("an item has text above it")
            .contains("Serialize")
    }

    #[test]
    fn no_source_file_makes_unexpected_serializable() {
        // A convention gate over text, not a proof: an unusual spelling would defeat it, and
        // the *structural* half of the guarantee — that a type without `Serialize` cannot be
        // handed to Tauri, and that `Rejection` has no field able to hold a message — is a
        // compile-time fact with no runtime red to demonstrate. What this catches is the
        // plausible mistake: somebody adding the derive to make an error "just work".
        let sources = crate_sources();
        // Assembled rather than written out, because this file is one of the files scanned and
        // a literal here would match itself.
        let forbidden = format!("{} for {}", "Serialize", "Unexpected");

        for (path, source) in &sources {
            assert!(
                !source.contains(&forbidden),
                "{path} implements {forbidden}"
            );
        }

        let this_file = sources
            .iter()
            .find(|(path, _)| path.ends_with("error.rs"))
            .expect("this file is among the crate's sources");
        assert!(!derives_serialize(&this_file.1, "pub struct Unexpected {"));

        // Negative control: the same reader must find a derive where one genuinely exists, or
        // the assertions above would pass against a scanner that sees nothing at all.
        let model = sources
            .iter()
            .find(|(path, _)| path.ends_with("state/model.rs"))
            .expect("the state model is among the crate's sources");
        assert!(derives_serialize(&model.1, "pub struct AppState {"));
    }

    #[test]
    fn unexpected_converts_into_any_failure_union() {
        #[derive(Debug)]
        enum SomeExpected {}
        let failure: Failure<SomeExpected> = Unexpected::new("boom").into();
        assert!(matches!(failure, Failure::Unexpected(_)));
    }
}
