//! Logical relative paths: the identity form for every file inside a Discovery Location
//! or Mod Source tree (docs/technical-design.md, "Installation identity").
//!
//! `/` separators, Unicode NFC, exact case-preserving comparison, no `.` or `..`
//! components, ordering by normalized UTF-8 bytes. Windows drive letters and root
//! prefixes never enter. Invalid Unicode is rejected without lossy conversion. Collision
//! visibility — two distinct raw entries normalizing to one logical path — is the
//! enumerating caller's job (Phase 2); this type makes it possible by being
//! deterministic.

use std::fmt;
use unicode_normalization::UnicodeNormalization;

/// An NFC-normalized, `/`-separated relative path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogicalPath(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    InvalidUnicode,
    Empty,
    AbsolutePrefix,
    /// Leading, trailing, or doubled separators.
    EmptyComponent,
    /// `.` or `..`.
    DotComponent,
    BackslashComponent,
    NulByte,
}

impl LogicalPath {
    /// Raw bytes from filesystem enumeration. Non-UTF-8 is rejected, never lossily
    /// converted.
    pub fn from_raw_bytes(raw: &[u8]) -> Result<Self, PathError> {
        let text = std::str::from_utf8(raw).map_err(|_| PathError::InvalidUnicode)?;
        Self::parse(text)
    }

    pub fn parse(raw: &str) -> Result<Self, PathError> {
        if raw.is_empty() {
            return Err(PathError::Empty);
        }
        if raw.contains('\0') {
            return Err(PathError::NulByte);
        }
        if raw.contains('\\') {
            return Err(PathError::BackslashComponent);
        }
        if raw.starts_with('/') || has_drive_prefix(raw) {
            return Err(PathError::AbsolutePrefix);
        }
        for component in raw.split('/') {
            if component.is_empty() {
                return Err(PathError::EmptyComponent);
            }
            if component == "." || component == ".." {
                return Err(PathError::DotComponent);
            }
        }
        Ok(Self(raw.nfc().collect()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

impl fmt::Display for LogicalPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn has_drive_prefix(raw: &str) -> bool {
    let mut chars = raw.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use unicode_normalization::is_nfc;

    #[test]
    fn nfd_and_nfc_input_normalize_to_the_same_path() {
        let decomposed = LogicalPath::parse("common/te\u{0301}ch.txt").unwrap();
        let composed = LogicalPath::parse("common/t\u{e9}ch.txt").unwrap();
        assert_eq!(decomposed, composed);
        assert!(is_nfc(decomposed.as_str()));
    }

    #[test]
    fn case_only_variants_remain_distinct() {
        let lower = LogicalPath::parse("common/tech.txt").unwrap();
        let upper = LogicalPath::parse("common/Tech.txt").unwrap();
        assert_ne!(lower, upper);
    }

    #[test]
    fn rejections() {
        assert_eq!(LogicalPath::parse(""), Err(PathError::Empty));
        assert_eq!(
            LogicalPath::parse("/common/a.txt"),
            Err(PathError::AbsolutePrefix)
        );
        assert_eq!(
            LogicalPath::parse("C:/mods/a.txt"),
            Err(PathError::AbsolutePrefix)
        );
        assert_eq!(
            LogicalPath::parse("a//b.txt"),
            Err(PathError::EmptyComponent)
        );
        assert_eq!(LogicalPath::parse("a/b/"), Err(PathError::EmptyComponent));
        assert_eq!(LogicalPath::parse("./a.txt"), Err(PathError::DotComponent));
        assert_eq!(
            LogicalPath::parse("a/../b.txt"),
            Err(PathError::DotComponent)
        );
        assert_eq!(
            LogicalPath::parse("a\\b.txt"),
            Err(PathError::BackslashComponent)
        );
        assert_eq!(LogicalPath::parse("a\0b"), Err(PathError::NulByte));
        assert_eq!(
            LogicalPath::from_raw_bytes(&[0x66, 0xff, 0x6f]),
            Err(PathError::InvalidUnicode)
        );
    }

    #[test]
    fn colon_is_only_special_as_a_drive_prefix() {
        assert!(LogicalPath::parse("events/a:b.txt").is_ok());
        assert_eq!(LogicalPath::parse("c:"), Err(PathError::AbsolutePrefix));
    }

    const PATH_RE: &str = "[a-zA-Z0-9._-]{1,12}(/[a-zA-Z0-9._-]{1,12}){0,4}";

    fn no_dot_components(raw: &str) -> bool {
        raw.split('/')
            .all(|component| component != "." && component != "..")
    }

    proptest! {
        #[test]
        fn parse_is_idempotent(raw in PATH_RE) {
            prop_assume!(no_dot_components(&raw));
            let first = LogicalPath::parse(&raw).unwrap();
            let second = LogicalPath::parse(first.as_str()).unwrap();
            prop_assert_eq!(first, second);
        }

        #[test]
        fn ordering_matches_normalized_utf8_bytes(a in PATH_RE, b in PATH_RE) {
            prop_assume!(no_dot_components(&a) && no_dot_components(&b));
            let left = LogicalPath::parse(&a).unwrap();
            let right = LogicalPath::parse(&b).unwrap();
            let by_bytes = left.as_str().as_bytes().cmp(right.as_str().as_bytes());
            prop_assert_eq!(left.cmp(&right), by_bytes);
        }
    }
}
