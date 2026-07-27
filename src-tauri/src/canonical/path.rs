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

impl fmt::Display for PathError {
    /// Prose, because enumeration shows this to a user as the reason a file was rejected.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidUnicode => "path is not valid Unicode",
            Self::Empty => "path is empty",
            Self::AbsolutePrefix => "path is absolute or carries a drive prefix",
            Self::EmptyComponent => "path has a leading, trailing, or doubled separator",
            Self::DotComponent => "path has a `.` or `..` relative component",
            Self::BackslashComponent => "path contains a backslash",
            Self::NulByte => "path contains a NUL byte",
        })
    }
}

impl std::error::Error for PathError {}

impl fmt::Display for LogicalPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for LogicalPath {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

/// Deserialization goes through [`LogicalPath::parse`], so a stored document that names
/// `../../etc/passwd` as an entry fails to decode. Traversal is therefore refused at the
/// boundary, before any validation stage gets the chance to open a path — the parse
/// result carries the evidence rather than leaving it to be re-checked downstream.
///
/// Written out rather than raised through `canonical::hex`'s `hex_string_serde!`: the
/// shape is the same, but the knowledge is not, and a macro named for hex must not
/// silently define what a path means. The rejection message is prose, matching
/// [`PathError`]'s reason strings.
impl<'de> serde::Deserialize<'de> for LogicalPath {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        // Names the offending value, so a rejected manifest entry is diagnosable from the
        // deserialize error alone rather than only by its position in the document.
        Self::parse(&text).map_err(|_| {
            serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&text),
                &"a relative NFC path with `/` separators and no `.` or `..` components",
            )
        })
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
    fn path_errors_render_a_reason_a_rejection_report_can_show() {
        // Source enumeration reports a rejected file to the user with its reason
        // (STE-11: "rejections are visible results"), so the reason must render as prose
        // rather than as a `{:?}` type name.
        fn assert_error<E: std::error::Error>(_: &E) {}
        assert_error(&PathError::InvalidUnicode);
        assert!(
            PathError::DotComponent
                .to_string()
                .contains("relative component")
        );
        for error in [
            PathError::InvalidUnicode,
            PathError::Empty,
            PathError::AbsolutePrefix,
            PathError::EmptyComponent,
            PathError::DotComponent,
            PathError::BackslashComponent,
            PathError::NulByte,
        ] {
            let rendered = error.to_string();
            assert!(!rendered.is_empty());
            assert!(!rendered.contains("PathError"), "{rendered}");
        }
    }

    #[test]
    fn a_logical_path_round_trips_as_a_plain_json_string() {
        // A revision manifest is human-readable JSON keyed and valued by logical paths
        // (docs/technical-design.md, "Materialized JSON read model"), so the stored form
        // must be the path text itself rather than a wrapper object.
        let path = LogicalPath::parse("common/technology/00_tech.txt").unwrap();
        assert_eq!(
            serde_json::to_string(&path).unwrap(),
            "\"common/technology/00_tech.txt\""
        );
        assert_eq!(
            serde_json::from_str::<LogicalPath>("\"common/technology/00_tech.txt\"").unwrap(),
            path
        );

        // Keys as well as values: entries are stored as a map from logical path.
        let map = std::collections::BTreeMap::from([(path.clone(), 1u32)]);
        let encoded = serde_json::to_string(&map).unwrap();
        assert_eq!(encoded, "{\"common/technology/00_tech.txt\":1}");
        assert_eq!(
            serde_json::from_str::<std::collections::BTreeMap<LogicalPath, u32>>(&encoded).unwrap(),
            map
        );
    }

    #[test]
    fn deserializing_normalizes_to_nfc_rather_than_preserving_stored_bytes() {
        // `parse` is the only way in, so a decomposed spelling in a stored document
        // becomes the same value a fresh enumeration would produce. Without that, a
        // hand-edited manifest could carry a path that never compares equal to the file
        // it names.
        let decomposed = serde_json::from_str::<LogicalPath>("\"common/te\u{301}ch.txt\"").unwrap();
        assert_eq!(
            decomposed,
            LogicalPath::parse("common/t\u{e9}ch.txt").unwrap()
        );
        assert!(is_nfc(decomposed.as_str()));
    }

    #[test]
    fn a_stored_traversal_path_fails_to_deserialize() {
        // The reason deserialization goes through `parse` at all: a manifest naming
        // `../../etc/passwd` as a required entry must be refused while it is still text,
        // so no validation stage ever resolves it against a real directory. Absolute
        // paths and backslashes are the same escape by another spelling.
        for hostile in [
            "\"../../etc/passwd\"",
            "\"common/../../etc/passwd\"",
            "\"/etc/passwd\"",
            "\"C:/Windows/System32/config/SAM\"",
            "\"..\\\\..\\\\etc\\\\passwd\"",
            "\"\"",
        ] {
            assert!(
                serde_json::from_str::<LogicalPath>(hostile).is_err(),
                "accepted {hostile}"
            );
        }

        // The rejection names the offending value, so the refusal is diagnosable from the
        // error alone rather than only from the document position.
        let error = serde_json::from_str::<LogicalPath>("\"../../etc/passwd\"").unwrap_err();
        assert!(error.to_string().contains("../../etc/passwd"), "{error}");
    }

    #[test]
    fn colon_is_only_special_as_a_drive_prefix() {
        assert!(LogicalPath::parse("events/a:b.txt").is_ok());
        assert_eq!(LogicalPath::parse("c:"), Err(PathError::AbsolutePrefix));
    }

    // Includes precomposed (é, ü) and combining (U+0301, U+0308) code points so the
    // properties exercise NFC normalization, not just ASCII pass-through.
    const PATH_RE: &str = "[a-zA-Z0-9._é\u{fc}\u{301}\u{308}-]{1,12}(/[a-zA-Z0-9._é\u{fc}\u{301}\u{308}-]{1,12}){0,4}";

    fn no_dot_components(raw: &str) -> bool {
        raw.split('/')
            .all(|component| component != "." && component != "..")
    }

    proptest! {
        #[test]
        fn parse_is_idempotent_and_nfc(raw in PATH_RE) {
            prop_assume!(no_dot_components(&raw));
            let first = LogicalPath::parse(&raw).unwrap();
            prop_assert!(is_nfc(first.as_str()));
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
