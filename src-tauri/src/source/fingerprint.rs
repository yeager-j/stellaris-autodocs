//! Content identity for a Mod Source: SHA-256 over the exact bytes presented for
//! analysis, and one domain-separated digest over the whole ordered set
//! (docs/technical-design.md, "Source module"; "Canonicalization and numeric
//! representation").
//!
//! A [`SourceFingerprint`] carries no mod identity, no absolute root, and no timestamps,
//! so the same value serves Target Mod and Vanilla Content alike; what distinguishes two
//! installations is the revision manifest that quotes them, not the fingerprint scheme.
//!
//! The framing is `canonical::encode`'s, which is tagged and length-prefixed, so no two
//! distinct file sets share a byte stream by concatenation:
//!
//! ```text
//! SHA-256( "stellaris-docs/source-fingerprint/v1" || 0x00
//!          || SEQ || u64be(count)
//!          || for each entry, ordered by logical-path bytes:
//!               TEXT  || u64be(len(path)) || path
//!               BYTES || u64be(32)        || sha256(content) )
//! ```
//!
//! Path and content are separate framed fields, so moving content between two files
//! changes the fingerprint. Logical paths are NFC and case-preserving, so a case-only
//! rename changes it on every platform, including the ones whose filesystems cannot tell
//! the two names apart.
//!
//! Change protocol: this is durable revision identity. Any change to the framing, the
//! hash, or what participates means a new domain version plus a bump of
//! `analysis::AnalysisVersionVector::source_enumeration`, never a re-pin of the golden
//! vector in place.

use crate::canonical::encode::{CanonicalDigest, DigestBytes};
use crate::canonical::path::LogicalPath;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

const FINGERPRINT_DOMAIN: &str = "stellaris-docs/source-fingerprint/v1";

/// Reading in bounded chunks keeps a multi-megabyte file off the heap in one piece; the
/// hash is over the same bytes either way.
const READ_CHUNK: usize = 64 * 1024;

/// SHA-256 of the exact bytes of one source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// Hashes the file's bytes as they are, with no encoding, newline, or BOM handling:
    /// analysis parses the same bytes this hashed.
    pub fn of_file(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; READ_CHUNK];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                return Ok(Self(hasher.finalize().into()));
            }
            hasher.update(&buffer[..read]);
        }
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// One value naming the complete content of a Mod Source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceFingerprint(DigestBytes);

/// Two source files claiming one logical path. Refused rather than folded: the
/// fingerprint would otherwise name one of two possible byte streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateLogicalPath {
    pub logical: LogicalPath,
}

impl fmt::Display for DuplicateLogicalPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "two source files claim the logical path {}",
            self.logical
        )
    }
}

impl std::error::Error for DuplicateLogicalPath {}

impl SourceFingerprint {
    /// Accepts entries in any order — enumeration order, completion order of parallel
    /// hashing — and imposes canonical logical-path order itself.
    pub fn of(
        entries: impl IntoIterator<Item = (LogicalPath, ContentHash)>,
    ) -> Result<Self, DuplicateLogicalPath> {
        let mut ordered: BTreeMap<LogicalPath, ContentHash> = BTreeMap::new();
        for (logical, content) in entries {
            // Any repeat is refused, including an identical one: a caller that offers a
            // path twice has lost track of its inventory, and quietly deduplicating
            // would hide that from the only stage that can see it.
            if ordered.insert(logical.clone(), content).is_some() {
                return Err(DuplicateLogicalPath { logical });
            }
        }
        let mut digest = CanonicalDigest::new(FINGERPRINT_DOMAIN);
        digest.begin_seq(ordered.len());
        for (logical, content) in &ordered {
            digest.text(logical.as_str()).bytes(&content.0);
        }
        Ok(Self(digest.finish()))
    }

    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }
}

impl fmt::Display for SourceFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn path(raw: &str) -> LogicalPath {
        LogicalPath::parse(raw).unwrap()
    }

    /// The pinned fixture set, mirrored by the Python derivation in the pinned test.
    fn pinned_entries() -> Vec<(LogicalPath, &'static [u8])> {
        vec![
            (
                path("common/technology/00_tech.txt"),
                b"tech_a = {}\n" as &[u8],
            ),
            (path("common/technology/01_tech.txt"), b"tech_b = {}\n"),
            (path("descriptor.mod"), b"name=\"Fixture\"\n"),
            (path("localisation/english/l_english.yml"), b"l_english:\n"),
        ]
    }

    fn hashed(entries: Vec<(LogicalPath, &[u8])>) -> Vec<(LogicalPath, ContentHash)> {
        entries
            .into_iter()
            .map(|(logical, bytes)| (logical, ContentHash::of(bytes)))
            .collect()
    }

    #[test]
    fn a_content_hash_is_sha256_of_the_exact_bytes() {
        // Independently derived: `printf 'tech_a = {}\n' | shasum -a 256`.
        assert_eq!(
            ContentHash::of(b"tech_a = {}\n").to_hex(),
            "5ed265f95312a78d906bb8fe36e15e70a9255ab701b02b82b55ba1a73e624ef2"
        );
        assert_eq!(
            ContentHash::of(b"").to_hex(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_ne!(ContentHash::of(b"a"), ContentHash::of(b"b"));
    }

    #[test]
    fn hashing_a_file_reads_its_exact_bytes() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("00_tech.txt");
        // Bytes, not text: a UTF-8 BOM and CRLF endings reach real script files, and
        // hashing must see them exactly as the parser will.
        fs::write(&file, b"\xef\xbb\xbftech_a = {}\r\n").unwrap();
        assert_eq!(
            ContentHash::of_file(&file).unwrap(),
            ContentHash::of(b"\xef\xbb\xbftech_a = {}\r\n")
        );
        assert!(ContentHash::of_file(&dir.path().join("absent.txt")).is_err());
    }

    #[test]
    fn hashing_a_large_file_matches_hashing_its_bytes() {
        // Exercises the streaming read across buffer boundaries.
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("big.txt");
        let bytes: Vec<u8> = (0..300_000u32).map(|index| (index % 251) as u8).collect();
        fs::write(&file, &bytes).unwrap();
        assert_eq!(
            ContentHash::of_file(&file).unwrap(),
            ContentHash::of(&bytes)
        );
    }

    #[test]
    fn pinned_fingerprint_vector() {
        // Pinned golden vector, derived independently of this implementation by a Python
        // script mirroring the documented framing (see the module comment):
        //
        //   SHA-256( "stellaris-docs/source-fingerprint/v1" || 0x00
        //            || 0x05 || u64be(n)
        //            || for each entry, ordered by logical-path bytes:
        //                 0x02 || u64be(len(path)) || path
        //                 0x01 || u64be(32)        || sha256(content) )
        //
        // Change protocol: a fingerprint is durable revision identity. A change to this
        // framing, to the digest, or to what participates is a NEW domain version
        // (/v2) plus a bump of AnalysisVersionVector::source_enumeration — never a
        // re-pin in place, which would silently redefine every stored revision's inputs.
        assert_eq!(
            SourceFingerprint::of(hashed(pinned_entries()))
                .unwrap()
                .to_hex(),
            "ca3b27102080255d98a3e869752493d33826c9584e3cbccf748b568cfd0dfb6e"
        );
        // An empty source is an identity, not an absence.
        assert_eq!(
            SourceFingerprint::of([]).unwrap().to_hex(),
            "a1f9564b4df9fdcda24c541ea103c8cfff3b37f24506d5a8693991fc2a1dce2d"
        );
    }

    #[test]
    fn input_order_does_not_change_the_fingerprint() {
        let ordered = hashed(pinned_entries());
        let mut shuffled = ordered.clone();
        shuffled.reverse();
        shuffled.swap(0, 2);
        assert_ne!(ordered, shuffled);
        assert_eq!(
            SourceFingerprint::of(ordered).unwrap(),
            SourceFingerprint::of(shuffled).unwrap()
        );
    }

    #[test]
    fn hashing_in_parallel_yields_the_same_fingerprint() {
        // Hashing is embarrassingly parallel and will be parallelized; the fingerprint
        // must be a function of the content, not of completion order.
        let entries = pinned_entries();
        let sequential = SourceFingerprint::of(hashed(entries.clone())).unwrap();

        let parallel: Vec<(LogicalPath, ContentHash)> = std::thread::scope(|scope| {
            let workers: Vec<_> = entries
                .iter()
                .map(|(logical, bytes)| {
                    scope.spawn(move || (logical.clone(), ContentHash::of(bytes)))
                })
                .collect();
            workers
                .into_iter()
                .rev()
                .map(|worker| worker.join().unwrap())
                .collect()
        });
        assert_eq!(SourceFingerprint::of(parallel).unwrap(), sequential);
    }

    #[test]
    fn a_changed_byte_changes_the_fingerprint() {
        let base = SourceFingerprint::of(hashed(pinned_entries())).unwrap();
        let mut edited = pinned_entries();
        edited[0].1 = b"tech_a = { cost = 1 }\n";
        assert_ne!(SourceFingerprint::of(hashed(edited)).unwrap(), base);
    }

    #[test]
    fn a_moved_path_changes_the_fingerprint_even_with_identical_content() {
        let base = SourceFingerprint::of(hashed(pinned_entries())).unwrap();
        let mut renamed = pinned_entries();
        renamed[0].0 = path("common/technology/02_tech.txt");
        assert_ne!(SourceFingerprint::of(hashed(renamed)).unwrap(), base);

        // Path and content are framed separately, so swapping content between two files
        // is not the same source as leaving them alone.
        let mut swapped = pinned_entries();
        swapped.swap(0, 1);
        let paths: Vec<LogicalPath> = pinned_entries()
            .into_iter()
            .map(|(logical, _)| logical)
            .collect();
        let crossed: Vec<(LogicalPath, ContentHash)> = paths
            .into_iter()
            .zip(swapped.into_iter().map(|(_, bytes)| ContentHash::of(bytes)))
            .collect();
        assert_ne!(SourceFingerprint::of(crossed).unwrap(), base);
    }

    #[test]
    fn duplicate_logical_paths_are_refused_rather_than_folded() {
        // A duplicate means the caller resolved a collision by picking a winner. The
        // fingerprint would then name one of two possible byte streams.
        let duplicated = vec![
            (path("common/a.txt"), ContentHash::of(b"one")),
            (path("common/a.txt"), ContentHash::of(b"two")),
        ];
        assert_eq!(
            SourceFingerprint::of(duplicated),
            Err(DuplicateLogicalPath {
                logical: path("common/a.txt")
            })
        );
        assert!(
            DuplicateLogicalPath {
                logical: path("common/a.txt")
            }
            .to_string()
            .contains("common/a.txt")
        );
    }

    #[test]
    fn the_fingerprint_domain_separates_it_from_other_identities() {
        // Same body bytes under another domain must not collide with a source
        // fingerprint (canonical::encode's domain-separation rule).
        let mut other = CanonicalDigest::new("stellaris-docs/source-fingerprint/v2");
        other.begin_seq(0);
        assert_ne!(
            SourceFingerprint::of([]).unwrap().to_hex(),
            other.finish().to_hex()
        );
    }
}
