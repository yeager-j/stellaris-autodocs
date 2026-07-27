//! Content identity for a Mod Source: SHA-256 over the exact bytes presented for
//! analysis, and one domain-separated digest over the whole ordered set
//! (docs/technical-design.md, "Source module"; "Canonicalization and numeric
//! representation").
//!
//! A [`SourceFingerprint`] carries no mod identity, no absolute root, and no timestamps,
//! so the same value serves Target Mod and Vanilla Content alike; what distinguishes two
//! installations is the revision manifest that quotes them, not the fingerprint scheme.
//!
//! A fingerprint covers what was observed **and what could not be**. An observation with
//! a normalization collision or a rejected entry is publishable — "evidence absent" is a
//! documented Incomplete Documentation condition, not a fatal one — but it is a different
//! source identity from the same content observed cleanly. Content alone would let a
//! dangling symlink be deleted, changing no enumerated file, and leave a revision
//! verifying as unchanged while it still carries stale "evidence absent" issues.
//!
//! The framing is `canonical::encode`'s, which is tagged and length-prefixed, so no two
//! distinct file sets share a byte stream by concatenation:
//!
//! ```text
//! SHA-256( "stellaris-docs/source-fingerprint/v3" || 0x00
//!          || SEQ || u64be(3)
//!          || SEQ || u64be(files)
//!          ||   for each file, ordered by logical-path bytes:
//!          ||     SEQ || u64be(2) || TEXT path || BYTES sha256(content)
//!          || SEQ || u64be(collisions)
//!          ||   for each collision, ordered by logical-path bytes:
//!          ||     SEQ || u64be(2) || TEXT logical
//!          ||                     || SEQ || u64be(k) || TEXT raw_label * k
//!          || SEQ || u64be(rejections)
//!          ||   for each rejection, ordered by (raw_label, reason_code):
//!          ||     SEQ || u64be(2) || TEXT raw_label || TEXT reason_code )
//! ```
//!
//! Each entry is a nested sequence because the enclosing `begin_seq(count)` promises
//! exactly `count` encoded items. v1 emitted a file's two fields flat, which stayed
//! unambiguous — the framing is length-prefixed — but contradicted the composite contract
//! `canonical::encode` states, so callers could not reason about the encoding from that
//! contract alone.
//!
//! Path and content are separate framed fields, so moving content between two files
//! changes the fingerprint. Logical paths are NFC and case-preserving, so a case-only
//! rename changes it on every platform, including the ones whose filesystems cannot tell
//! the two names apart.
//!
//! **Only source-determined fields enter.** A gap contributes its logical or raw label and
//! [`RejectionReason::code`](crate::source::RejectionReason::code) — never an OS message,
//! an `io::ErrorKind`, or a canonicalized absolute escape target, each of which describes
//! the machine that did the observing. That is what keeps one mod's revision identifier
//! the same on two machines; the reasoning for each exclusion is recorded on `code`.
//!
//! Change protocol: this is durable revision identity. Any change to the framing, the
//! hash, or what participates means a new domain version plus a bump of
//! `analysis::AnalysisVersionVector::source_enumeration`, never a re-pin of the golden
//! vector in place.

use crate::canonical::encode::{CanonicalDigest, DigestBytes};
use crate::canonical::path::LogicalPath;
use crate::source::enumerate::ObservationGaps;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

const FINGERPRINT_DOMAIN: &str = "stellaris-docs/source-fingerprint/v3";

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

    /// Reads back a rendered hash. Strict: exactly 64 lowercase hex characters, so one
    /// stored form has one parse (`discovery::identity`'s rule).
    pub fn parse(text: &str) -> Result<Self, HashParseError> {
        decode_hex(text).map(Self).ok_or(HashParseError)
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
    /// Names one observation of a source: the exact content it captured together with the
    /// gaps that stopped it from capturing more.
    ///
    /// Accepts entries in any order — enumeration order, completion order of parallel
    /// hashing — and imposes canonical order on all three sets itself, so the value is a
    /// function of the observation rather than of how it was assembled. `gaps` is taken by
    /// reference and read, never sorted in place: the caller's report keeps its own order.
    pub fn of(
        entries: impl IntoIterator<Item = (LogicalPath, ContentHash)>,
        gaps: &ObservationGaps,
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

        // Every item below is one item of its enclosing sequence, so each is its own
        // nested sequence: `begin_seq(n)` promises exactly n encoded items
        // (canonical::encode's composite framing contract).
        let mut digest = CanonicalDigest::new(FINGERPRINT_DOMAIN);
        digest.begin_seq(3);

        digest.begin_seq(ordered.len());
        for (logical, content) in &ordered {
            digest.begin_seq(2).text(logical.as_str()).bytes(&content.0);
        }

        let collisions = ordered_collisions(gaps);
        digest.begin_seq(collisions.len());
        for (logical, labels) in &collisions {
            digest.begin_seq(2).text(logical);
            digest.begin_seq(labels.len());
            for label in labels {
                digest.text(label);
            }
        }

        let rejections = ordered_rejections(gaps);
        digest.begin_seq(rejections.len());
        for (label, code) in &rejections {
            digest.begin_seq(2).text(label).text(code);
        }

        Ok(Self(digest.finish()))
    }

    /// Reads back a rendered fingerprint, so a stored revision input can be compared with
    /// a freshly computed one.
    pub fn parse(text: &str) -> Result<Self, HashParseError> {
        decode_hex(text)
            .map(|bytes| Self(DigestBytes(bytes)))
            .ok_or(HashParseError)
    }

    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }
}

/// The collision half of the gap projection, in canonical order.
///
/// Both levels are sorted here rather than trusted from the caller, for the same reason
/// [`SourceFingerprint::of`] sorts files: the digest must be a function of the observation,
/// not of the order a walk happened to assemble it in. `classify_entries` already produces
/// both orders, so this re-sort costs nothing on the real path and removes the dependence.
///
/// Ordering is over the whole projected tuple, not the logical path alone, so the order is
/// total even for input that repeats a logical path — which `classify_entries` cannot
/// produce, but `of` accepts entries from any caller.
fn ordered_collisions(gaps: &ObservationGaps) -> Vec<(&str, Vec<&str>)> {
    let mut collisions: Vec<(&str, Vec<&str>)> = gaps
        .collisions
        .iter()
        .map(|collision| {
            let mut labels: Vec<&str> = collision.raw_labels.iter().map(String::as_str).collect();
            labels.sort_unstable();
            (collision.logical.as_str(), labels)
        })
        .collect();
    collisions.sort_unstable();
    collisions
}

/// The rejection half of the gap projection, in canonical order.
///
/// A rejection projects to `(raw_label, reason_code)` and nothing else. Two rejections
/// that project equal encode identically, so their relative order cannot matter — which is
/// what lets a stable order exist at all for the repeated labels a walk can emit (several
/// unreadable entries in one directory share a label).
fn ordered_rejections(gaps: &ObservationGaps) -> Vec<(&str, &'static str)> {
    let mut rejections: Vec<(&str, &'static str)> = gaps
        .rejected
        .iter()
        .map(|rejection| (rejection.raw_label.as_str(), rejection.reason.code()))
        .collect();
    rejections.sort_unstable();
    rejections
}

/// A rendered hash that is not exactly 64 lowercase hex characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashParseError;

impl fmt::Display for HashParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid hash: expected 64 lowercase hex characters")
    }
}

impl std::error::Error for HashParseError {}

/// Uppercase is refused rather than accepted-and-normalized: two spellings of one hash
/// would make stored manifest text ambiguous. Mirrors `discovery::identity::decode_hex`;
/// a shared codec is worth extracting once a third module needs one.
fn decode_hex(text: &str) -> Option<[u8; 32]> {
    let bytes = text.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(bytes.chunks_exact(2)) {
        *slot = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(out)
}

impl fmt::Display for SourceFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::path::PathError;
    use crate::source::enumerate::{FileCollision, RejectedFile, RejectionReason};
    use std::fs;
    use tempfile::TempDir;

    fn path(raw: &str) -> LogicalPath {
        LogicalPath::parse(raw).unwrap()
    }

    fn complete() -> ObservationGaps {
        ObservationGaps::default()
    }

    fn rejection(raw_label: &str, reason: RejectionReason) -> RejectedFile {
        RejectedFile {
            raw_label: raw_label.to_owned(),
            reason,
        }
    }

    /// One collision and one rejection of every stable reason code, mirrored by the
    /// Python derivation in the pinned test.
    fn pinned_gaps() -> ObservationGaps {
        ObservationGaps {
            collisions: vec![FileCollision {
                logical: path("common/technology/t\u{e9}ch.txt"),
                raw_labels: vec![
                    "common/technology/te\u{301}ch.txt".to_owned(),
                    "common/technology/t\u{e9}ch.txt".to_owned(),
                ],
            }],
            rejected: vec![
                rejection(
                    "common/technology/bad\u{fffd}name.txt",
                    RejectionReason::InvalidUnicode,
                ),
                rejection(
                    "common/technology/",
                    RejectionReason::InvalidPath(PathError::Empty),
                ),
                rejection(
                    "C:/technology/drive.txt",
                    RejectionReason::InvalidPath(PathError::AbsolutePrefix),
                ),
                rejection(
                    "common//technology/doubled.txt",
                    RejectionReason::InvalidPath(PathError::EmptyComponent),
                ),
                rejection(
                    "common/technology/../dot.txt",
                    RejectionReason::InvalidPath(PathError::DotComponent),
                ),
                rejection(
                    "common/technology/back\\slash.txt",
                    RejectionReason::InvalidPath(PathError::BackslashComponent),
                ),
                rejection(
                    "common/technology/nul\u{0}byte.txt",
                    RejectionReason::InvalidPath(PathError::NulByte),
                ),
                rejection(
                    "common/technology/undecodable.txt",
                    RejectionReason::InvalidPath(PathError::InvalidUnicode),
                ),
                rejection(
                    "common/technology/escaped.txt",
                    RejectionReason::TraversalEscape {
                        target: "/Users/someone/elsewhere/foreign".to_owned(),
                    },
                ),
                rejection("common/technology/loop", RejectionReason::SymlinkCycle),
                rejection(
                    "common/technology/dangling.txt",
                    RejectionReason::Unreadable {
                        kind: io::ErrorKind::NotFound,
                        detail: "No such file or directory (os error 2)".to_owned(),
                    },
                ),
            ],
        }
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
        // Pinned golden vectors, derived independently of this implementation by a Python
        // script mirroring the documented framing (see the module comment):
        //
        //   SHA-256( "stellaris-docs/source-fingerprint/v3" || 0x00
        //            || 0x05 || u64be(3)
        //            || 0x05 || u64be(files)
        //            ||   0x05 || u64be(2)
        //            ||   0x02 || u64be(len(path))  || path
        //            ||   0x01 || u64be(32)         || sha256(content)
        //            || 0x05 || u64be(collisions)
        //            ||   0x05 || u64be(2)
        //            ||   0x02 || u64be(len(logical)) || logical
        //            ||   0x05 || u64be(k)
        //            ||     0x02 || u64be(len(label)) || label
        //            || 0x05 || u64be(rejections)
        //            ||   0x05 || u64be(2)
        //            ||   0x02 || u64be(len(label))   || label
        //            ||   0x02 || u64be(len(code))    || code )
        //
        // Change protocol: a fingerprint is durable revision identity. A change to this
        // framing, to the digest, or to what participates is a NEW domain version plus a
        // bump of AnalysisVersionVector::source_enumeration — never a re-pin in place,
        // which would silently redefine every stored revision's inputs.
        //
        // v1 flattened each file into the outer sequence: it declared n items and then
        // emitted 2n, which broke canonical::encode's stated composite contract even
        // though its length-prefixed framing stayed unambiguous. v2 gave each file its own
        // two-item sequence. v3 adds the observation gaps, because content alone let a
        // source stop being broken without changing identity: deleting a dangling symlink
        // removes a rejection and touches no enumerated file. Each bump follows the
        // protocol above rather than a re-pin; no migration accompanies them because no
        // fingerprint has been persisted — revision publication arrives in Phase 3.
        assert_eq!(
            SourceFingerprint::of(hashed(pinned_entries()), &complete())
                .unwrap()
                .to_hex(),
            "35cce5a4c7e25e08dfa0a327d5a5f5634b58adbbfb4f9f2d1c6e0282a2eb4da7"
        );
        // An empty source is an identity, not an absence.
        assert_eq!(
            SourceFingerprint::of([], &complete()).unwrap().to_hex(),
            "4e57c37e73936883be57f0b5dbb972b10e0d40dca1fd4fa785e622f8cc54bc4b"
        );
        // The same content observed through a walk that hit one collision and one
        // rejection of every reason code. This vector is what pins the code strings into
        // durable identity.
        assert_eq!(
            SourceFingerprint::of(hashed(pinned_entries()), &pinned_gaps())
                .unwrap()
                .to_hex(),
            "4e1d7a1b53340326ada23c09685075609a085807f8a619d21c51f44849829a47"
        );
    }

    #[test]
    fn gaps_change_the_fingerprint_of_identical_content() {
        // The reason /v3 exists. Same four files, three observations: clean, one
        // collision, one rejection. All three must be distinct identities.
        let content = hashed(pinned_entries());
        let clean = SourceFingerprint::of(content.clone(), &complete()).unwrap();
        let collided = SourceFingerprint::of(
            content.clone(),
            &ObservationGaps {
                collisions: pinned_gaps().collisions,
                rejected: Vec::new(),
            },
        )
        .unwrap();
        let rejected = SourceFingerprint::of(
            content,
            &ObservationGaps {
                collisions: Vec::new(),
                rejected: vec![rejection(
                    "common/technology/dangling.txt",
                    RejectionReason::Unreadable {
                        kind: io::ErrorKind::NotFound,
                        detail: "No such file or directory (os error 2)".to_owned(),
                    },
                )],
            },
        )
        .unwrap();
        assert_ne!(clean, collided);
        assert_ne!(clean, rejected);
        assert_ne!(collided, rejected);
    }

    #[test]
    fn each_reason_code_is_a_distinct_identity() {
        // A gap set is not merely "n things went wrong": which thing went wrong is part
        // of the source's identity, so the codes must not collapse into each other.
        let codes = [
            RejectionReason::InvalidUnicode,
            RejectionReason::InvalidPath(PathError::DotComponent),
            RejectionReason::InvalidPath(PathError::BackslashComponent),
            RejectionReason::TraversalEscape {
                target: "/elsewhere".to_owned(),
            },
            RejectionReason::SymlinkCycle,
            RejectionReason::Unreadable {
                kind: io::ErrorKind::NotFound,
                detail: "gone".to_owned(),
            },
        ];
        let fingerprints: std::collections::BTreeSet<String> = codes
            .into_iter()
            .map(|reason| {
                SourceFingerprint::of(
                    [],
                    &ObservationGaps {
                        collisions: Vec::new(),
                        rejected: vec![rejection("common/a.txt", reason)],
                    },
                )
                .unwrap()
                .to_hex()
            })
            .collect();
        assert_eq!(fingerprints.len(), 6);
    }

    #[test]
    fn the_gap_projection_ignores_host_dependent_payloads() {
        // The property the whole gap projection exists to satisfy: two machines observing
        // one broken mod must agree on its identity. They will not agree on the absolute
        // path a link escaped to, on the OS error text, or even on the error kind — a file
        // that is missing here may be unreadable there.
        let here = ObservationGaps {
            collisions: Vec::new(),
            rejected: vec![
                rejection(
                    "common/escaped.txt",
                    RejectionReason::TraversalEscape {
                        target: "/Users/jackson/Library/Steam/foreign".to_owned(),
                    },
                ),
                rejection(
                    "common/dangling.txt",
                    RejectionReason::Unreadable {
                        kind: io::ErrorKind::NotFound,
                        detail: "No such file or directory (os error 2)".to_owned(),
                    },
                ),
            ],
        };
        let there = ObservationGaps {
            collisions: Vec::new(),
            rejected: vec![
                rejection(
                    "common/escaped.txt",
                    RejectionReason::TraversalEscape {
                        target: "/var/lib/steam/somewhere-else-entirely".to_owned(),
                    },
                ),
                rejection(
                    "common/dangling.txt",
                    RejectionReason::Unreadable {
                        kind: io::ErrorKind::PermissionDenied,
                        detail: "Permission denied (os error 13)".to_owned(),
                    },
                ),
            ],
        };
        assert_ne!(here, there);
        assert_eq!(
            SourceFingerprint::of([], &here).unwrap(),
            SourceFingerprint::of([], &there).unwrap()
        );
    }

    #[test]
    fn gap_order_does_not_change_the_fingerprint() {
        // `of` imposes canonical order on gaps for the same reason it imposes it on
        // files: a report assembled in walk order must not become part of identity.
        let ordered = pinned_gaps();
        let mut shuffled = ordered.clone();
        shuffled.rejected.reverse();
        shuffled.rejected.swap(0, 3);
        shuffled.collisions[0].raw_labels.reverse();
        assert_ne!(ordered, shuffled);
        assert_eq!(
            SourceFingerprint::of(hashed(pinned_entries()), &ordered).unwrap(),
            SourceFingerprint::of(hashed(pinned_entries()), &shuffled).unwrap()
        );
    }

    #[test]
    fn a_repeated_rejection_is_counted_not_folded() {
        // A walk can emit one label twice — several unreadable entries in one directory
        // share `<dir>/(unreadable directory entry)`. Two gaps are not one gap.
        let once = ObservationGaps {
            collisions: Vec::new(),
            rejected: vec![rejection(
                "common/(unreadable directory entry)",
                RejectionReason::Unreadable {
                    kind: io::ErrorKind::PermissionDenied,
                    detail: "permission denied".to_owned(),
                },
            )],
        };
        let mut twice = once.clone();
        twice.rejected.push(once.rejected[0].clone());
        assert_ne!(
            SourceFingerprint::of([], &once).unwrap(),
            SourceFingerprint::of([], &twice).unwrap()
        );
    }

    #[test]
    fn hashes_and_fingerprints_round_trip_through_their_rendered_form() {
        // A revision manifest stores these as text and must read them back to re-check a
        // prior build's inputs; a write-only identity cannot be compared with a stored
        // one. Strict lowercase hex of the exact length, mirroring discovery::identity.
        let content = ContentHash::of(b"tech_a = {}\n");
        assert_eq!(ContentHash::parse(&content.to_hex()), Ok(content));
        let fingerprint = SourceFingerprint::of(hashed(pinned_entries()), &complete()).unwrap();
        assert_eq!(
            SourceFingerprint::parse(&fingerprint.to_hex()),
            Ok(fingerprint)
        );

        assert_eq!(ContentHash::parse(""), Err(HashParseError));
        assert_eq!(ContentHash::parse("abc"), Err(HashParseError));
        assert_eq!(
            ContentHash::parse(&content.to_hex().to_uppercase()),
            Err(HashParseError)
        );
        assert_eq!(
            ContentHash::parse(&format!("{}zz", &content.to_hex()[..62])),
            Err(HashParseError)
        );
        assert_eq!(
            SourceFingerprint::parse(&fingerprint.to_hex()[..63]),
            Err(HashParseError)
        );

        fn assert_error<E: std::error::Error>(_: &E) {}
        assert_error(&HashParseError);
        assert!(!HashParseError.to_string().is_empty());
    }

    #[test]
    fn input_order_does_not_change_the_fingerprint() {
        // Over a gap-bearing observation, so the property covers all three sets rather
        // than only the files.
        let ordered = hashed(pinned_entries());
        let mut shuffled = ordered.clone();
        shuffled.reverse();
        shuffled.swap(0, 2);
        assert_ne!(ordered, shuffled);
        assert_eq!(
            SourceFingerprint::of(ordered, &pinned_gaps()).unwrap(),
            SourceFingerprint::of(shuffled, &pinned_gaps()).unwrap()
        );
    }

    #[test]
    fn hashing_in_parallel_yields_the_same_fingerprint() {
        // Hashing is embarrassingly parallel and will be parallelized; the fingerprint
        // must be a function of the content, not of completion order.
        let entries = pinned_entries();
        let sequential = SourceFingerprint::of(hashed(entries.clone()), &pinned_gaps()).unwrap();

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
        assert_eq!(
            SourceFingerprint::of(parallel, &pinned_gaps()).unwrap(),
            sequential
        );
    }

    #[test]
    fn a_changed_byte_changes_the_fingerprint() {
        let base = SourceFingerprint::of(hashed(pinned_entries()), &complete()).unwrap();
        let mut edited = pinned_entries();
        edited[0].1 = b"tech_a = { cost = 1 }\n";
        assert_ne!(
            SourceFingerprint::of(hashed(edited), &complete()).unwrap(),
            base
        );
    }

    #[test]
    fn a_moved_path_changes_the_fingerprint_even_with_identical_content() {
        let base = SourceFingerprint::of(hashed(pinned_entries()), &complete()).unwrap();
        let mut renamed = pinned_entries();
        renamed[0].0 = path("common/technology/02_tech.txt");
        assert_ne!(
            SourceFingerprint::of(hashed(renamed), &complete()).unwrap(),
            base
        );

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
        assert_ne!(SourceFingerprint::of(crossed, &complete()).unwrap(), base);
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
            SourceFingerprint::of(duplicated, &complete()),
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
        // fingerprint (canonical::encode's domain-separation rule). The stand-in is the
        // next version of this very domain: a future scheme change must not produce a
        // value that an older reader would accept as its own.
        let mut other = CanonicalDigest::new("stellaris-docs/source-fingerprint/v3");
        other.begin_seq(0);
        assert_ne!(
            SourceFingerprint::of([], &complete()).unwrap().to_hex(),
            other.finish().to_hex()
        );
    }
}
