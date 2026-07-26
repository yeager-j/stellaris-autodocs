//! SHA-256, in the lowercase hexadecimal form every record in this repository uses.
//!
//! Restated rather than imported from `dds_spike::digest`. It is thirty lines of algorithm
//! with no decision in it, and importing it would make this crate's record format depend on
//! another spike's for no gain — the coupling that would matter, the adopted parser and
//! decoder, is a path dependency precisely because it *does* carry decisions.

use sha2::{Digest, Sha256};

pub fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A digest over an ordered stream of `(name, digest)` pairs.
///
/// One value that changes if any member changes: a corpus tree, a bundle's required entries,
/// a revision's asset key set.
pub struct Stream(Sha256);

impl Stream {
    pub fn new() -> Self {
        Self(Sha256::new())
    }

    pub fn push(&mut self, name: &str, digest: &str) {
        self.0.update(name.as_bytes());
        self.0.update([0]);
        self.0.update(digest.as_bytes());
        self.0.update([b'\n']);
    }

    pub fn finish(self) -> String {
        hex(&self.0.finalize())
    }
}

impl Default for Stream {
    fn default() -> Self {
        Self::new()
    }
}

/// A digest with an explicit domain separator, for values that become identities.
///
/// `docs/technical-design.md:330` requires a domain separator on the Revision identifier and
/// on the asset key so that two different canonical bodies cannot collide by being byte-equal
/// in different roles.
pub fn domain_separated(domain: &str, body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    hasher.update([0]);
    hasher.update(body);
    hex(&hasher.finalize())
}
