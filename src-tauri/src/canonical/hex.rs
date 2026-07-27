//! The crate's one lowercase-hex codec, and the serde shape every hex-string identity
//! shares.
//!
//! Rendered hex is the stored form of every fixed-width identity in the app: Discovery
//! Location and Mod Installation identifiers, content hashes, source fingerprints, and
//! the digests underneath them. Two independent copies of the codec existed before a
//! third caller (the revision manifest) needed one, which is the point at which the rule
//! was worth owning in one place.
//!
//! **Uppercase is refused rather than accepted-and-normalized.** Two spellings of one
//! hash would make stored manifest and state text ambiguous: the same identity would
//! compare unequal as text while comparing equal after parsing, and nothing downstream
//! could say which spelling was canonical. One stored form has one parse.
//!
//! Like `canonical::encode`, this is a leaf primitive below the deep-module row. It owns
//! the mechanics only: each identity's width, domain, and schema stay owned by the module
//! that defines that identity.

use std::fmt::{self, Write};

/// Appends `bytes` as lowercase hex. Takes any [`fmt::Write`] so a `Display` impl can
/// render without allocating an intermediate `String`.
pub fn write(out: &mut impl Write, bytes: &[u8]) -> fmt::Result {
    for byte in bytes {
        write!(out, "{byte:02x}")?;
    }
    Ok(())
}

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    // `String`'s `fmt::Write` impl never returns Err, so there is no failure to report.
    write(&mut out, bytes).expect("writing hex into a String is infallible");
    out
}

/// Reads exactly `N * 2` lowercase hex characters. Const-generic because the widths in
/// use differ: a Discovery Location identifier is 16 bytes, everything digest-valued is
/// 32.
///
/// Returns `None` for the wrong length, for uppercase, and for any non-hex byte. The
/// caller maps that to its own parse error, because what a malformed value means — a
/// corrupt state file, a hand-edited manifest — is the caller's knowledge, not this
/// module's.
pub fn decode<const N: usize>(text: &str) -> Option<[u8; N]> {
    let bytes = text.as_bytes();
    if bytes.len() != N * 2 {
        return None;
    }
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
    let mut out = [0u8; N];
    for (slot, pair) in out.iter_mut().zip(bytes.chunks_exact(2)) {
        let hi = nibble(pair[0])?;
        let lo = nibble(pair[1])?;
        *slot = (hi << 4) | lo;
    }
    Some(out)
}

/// Serde for a hex-string identity: `Display` out, the type's own `parse` in.
///
/// The stored form is the rendered hex string rather than a byte array or an object, so
/// a state file or revision manifest stays readable and diffable, and so the same text a
/// log line prints is the text a document holds. `$expected` is the shape a reader
/// should have seen; it is the type's own knowledge, hence a parameter.
macro_rules! hex_string_serde {
    ($ty:ident, $expected:literal) => {
        impl serde::Serialize for $ty {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }
        impl<'de> serde::Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let text = String::deserialize(d)?;
                // Names the offending value (not just the shape it should have had), so a
                // corrupted state file is diagnosable from the deserialize error alone.
                $ty::parse(&text).map_err(|_| {
                    serde::de::Error::invalid_value(serde::de::Unexpected::Str(&text), &$expected)
                })
            }
        }
    };
}

pub(crate) use hex_string_serde;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_renders_lowercase_pairs_including_leading_zeros() {
        // A byte below 0x10 must render as two characters: dropping the leading zero
        // would shorten the string and make two distinct byte arrays share a spelling.
        assert_eq!(encode(&[0x00, 0x0f, 0xff, 0xa0]), "000fffa0");
        assert_eq!(encode(&[]), "");
    }

    #[test]
    fn writing_and_encoding_agree() {
        // `Display` impls take the non-allocating path and `to_hex` takes the allocating
        // one; the two must not be able to disagree about a value's spelling.
        let bytes: Vec<u8> = (0..=255u8).collect();
        let mut written = String::new();
        write(&mut written, &bytes).unwrap();
        assert_eq!(written, encode(&bytes));
    }

    #[test]
    fn decoding_round_trips_every_byte_value() {
        let bytes: [u8; 256] = std::array::from_fn(|index| index as u8);
        assert_eq!(decode::<256>(&encode(&bytes)), Some(bytes));
    }

    #[test]
    fn uppercase_is_refused_rather_than_normalized() {
        // The rule the module exists to hold in one place: accepting uppercase would give
        // one identity two stored spellings, so a manifest could name the same hash twice
        // and compare it unequal as text.
        assert_eq!(decode::<2>("00ff"), Some([0x00, 0xff]));
        assert_eq!(decode::<2>("00FF"), None);
        assert_eq!(decode::<2>("00Ff"), None);
    }

    #[test]
    fn a_wrong_length_or_a_non_hex_byte_is_refused() {
        assert_eq!(decode::<2>(""), None);
        assert_eq!(decode::<2>("00f"), None);
        assert_eq!(decode::<2>("00ff00"), None);
        assert_eq!(decode::<2>("00fg"), None);
        // Multi-byte UTF-8 makes the byte length differ from the character count; the
        // length check is over bytes, so this is refused on length before any nibble.
        assert_eq!(decode::<2>("00é"), None);
    }
}
