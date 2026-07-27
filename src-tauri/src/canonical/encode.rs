//! Domain-separated canonical digests over a tagged, length-prefixed encoding.
//!
//! Every stable identity (fingerprints, Revision identifiers, Hidden Route identities,
//! asset keys) hashes this encoding rather than serializer output or map iteration order
//! (docs/technical-design.md, "Canonicalization and numeric representation"). Framing is
//! tagged and length-prefixed so no two distinct values share a byte stream by
//! concatenation — the same rule the parser spike's digest proved out. The encoding is
//! one-way: nothing decodes it, so evolution means a new domain version and a bumped
//! [`ENCODING_VERSION`], never in-place reinterpretation.
//!
//! Composite framing contract (enforced by callers, stated here once):
//! - `begin_seq(len)` is followed by exactly `len` encoded items.
//! - `begin_map(len)` is followed by `len` key–value pairs, keys already in canonical
//!   UTF-8 byte order; iterating a `BTreeMap<String, _>` satisfies this.
//! - `some()` is followed by exactly one encoded value; `none()` stands alone.

use sha2::{Digest, Sha256};
use std::fmt;

/// Encoding format version. Participates in the analysis version vector as its
/// `canonical_encoding` component.
pub const ENCODING_VERSION: u32 = 1;

mod tag {
    pub const BYTES: u8 = 0x01;
    pub const TEXT: u8 = 0x02;
    pub const U64: u8 = 0x03;
    pub const BOOL: u8 = 0x04;
    pub const SEQ: u8 = 0x05;
    pub const MAP: u8 = 0x06;
    pub const NONE: u8 = 0x07;
    pub const SOME: u8 = 0x08;
}

/// A finished 32-byte canonical digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DigestBytes(pub [u8; 32]);

impl DigestBytes {
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Display for DigestBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

pub struct CanonicalDigest {
    hasher: Sha256,
}

impl CanonicalDigest {
    /// `domain` names the identity and its version, e.g. `stellaris-docs/asset-key/v1`.
    pub fn new(domain: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(domain.as_bytes());
        hasher.update([0]);
        Self { hasher }
    }

    pub fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.hasher.update([tag::BYTES]);
        self.len(value.len());
        self.hasher.update(value);
        self
    }

    pub fn text(&mut self, value: &str) -> &mut Self {
        self.hasher.update([tag::TEXT]);
        self.len(value.len());
        self.hasher.update(value.as_bytes());
        self
    }

    pub fn u64(&mut self, value: u64) -> &mut Self {
        self.hasher.update([tag::U64]);
        self.hasher.update(value.to_be_bytes());
        self
    }

    pub fn bool(&mut self, value: bool) -> &mut Self {
        self.hasher.update([tag::BOOL, u8::from(value)]);
        self
    }

    pub fn begin_seq(&mut self, len: usize) -> &mut Self {
        self.hasher.update([tag::SEQ]);
        self.len(len);
        self
    }

    pub fn begin_map(&mut self, len: usize) -> &mut Self {
        self.hasher.update([tag::MAP]);
        self.len(len);
        self
    }

    pub fn none(&mut self) -> &mut Self {
        self.hasher.update([tag::NONE]);
        self
    }

    pub fn some(&mut self) -> &mut Self {
        self.hasher.update([tag::SOME]);
        self
    }

    pub fn finish(self) -> DigestBytes {
        DigestBytes(self.hasher.finalize().into())
    }

    fn len(&mut self, len: usize) {
        self.hasher.update((len as u64).to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest_of(build: impl FnOnce(&mut CanonicalDigest)) -> DigestBytes {
        let mut digest = CanonicalDigest::new("stellaris-docs/encode-test/v1");
        build(&mut digest);
        digest.finish()
    }

    #[test]
    fn framing_distinguishes_concatenation_from_separate_items() {
        let joined = digest_of(|d| {
            d.begin_seq(1).text("ab");
        });
        let split = digest_of(|d| {
            d.begin_seq(2).text("a").text("b");
        });
        assert_ne!(joined, split);
    }

    #[test]
    fn framing_distinguishes_types_with_identical_bytes() {
        let text = digest_of(|d| {
            d.text("a");
        });
        let bytes = digest_of(|d| {
            d.bytes(b"a");
        });
        assert_ne!(text, bytes);
    }

    #[test]
    fn domains_separate_identical_bodies() {
        let mut first = CanonicalDigest::new("stellaris-docs/a/v1");
        first.u64(1);
        let mut second = CanonicalDigest::new("stellaris-docs/b/v1");
        second.u64(1);
        assert_ne!(first.finish(), second.finish());
    }

    #[test]
    fn encoded_none_differs_from_absence() {
        let with_none = digest_of(|d| {
            d.u64(1).none();
        });
        let absent = digest_of(|d| {
            d.u64(1);
        });
        assert_ne!(with_none, absent);
    }

    #[test]
    fn pinned_self_test_digest() {
        // Pinned regression value. Any framing or tag change must consciously bump
        // ENCODING_VERSION (and the analysis version vector's canonical_encoding
        // component), then re-pin this value.
        let mut digest = CanonicalDigest::new("stellaris-docs/canonical-selftest/v1");
        digest
            .text("tech_lasers_1")
            .u64(3)
            .bool(true)
            .begin_seq(2)
            .text("a")
            .text("b")
            .begin_map(1)
            .text("k")
            .u64(7)
            .none()
            .some()
            .u64(9)
            .bytes(&[0x00, 0xff]);
        assert_eq!(
            digest.finish().to_hex(),
            "92545b5ae23bba69580a855a9ac49c9422b60cd45f108e6ae752b4c201216581"
        );
    }
}
