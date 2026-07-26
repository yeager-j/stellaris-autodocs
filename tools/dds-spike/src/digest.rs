//! SHA-256, in the lowercase hexadecimal form every record in this repository uses.

use sha2::{Digest, Sha256};

pub fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A digest over an ordered stream of `(name, digest)` pairs.
///
/// Used wherever a set of things needs one value that changes if any member changes: a corpus
/// tree, a run's decoded outputs, a fixture directory.
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
