//! Content-addressed localization chunking, and how much cross-revision duplication it
//! recovers.
//!
//! `docs/technical-design.md:349` pre-authorizes an immutable content-addressed Localization
//! Store as the response *if* preserved all-language localization turns out to dominate
//! cross-revision duplication. This module measures whether it does, and how much a store
//! would actually recover — which is a different question, and the one that decides whether
//! the machinery earns its place.
//!
//! Two chunkings are measured rather than one, because the naive answer is misleading.
//!
//! **Whole-language chunks.** One chunk per language table. Trivially correct, and it dedupes
//! only languages a mod never touches at all. It is the floor.
//!
//! **Content-defined chunks.** Boundaries chosen by the *content* rather than by position, so
//! inserting a key shifts one chunk instead of renumbering every chunk after it. This is the
//! whole reason content-addressed chunking is worth more than fixed-size blocks: mods almost
//! always add keys, and a fixed-size scheme over a sorted key space would re-cut every block
//! after the first insertion and dedupe close to nothing.
//!
//! The store is not built here. Measure first; `docs/technical-design.md:349` makes the store
//! conditional on the measurement, and a store built before the number is a mechanism looking
//! for a justification.

use crate::digest::sha256;
use crate::localization::Localization;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Average target entries per content-defined chunk.
///
/// A power of two so the boundary test is a mask. Sized so a language table of ~150,000 keys
/// yields a few hundred chunks: small enough that one mod's additions dirty a small fraction,
/// large enough that per-chunk overhead stays negligible against a ~1 KiB payload.
const TARGET_CHUNK_ENTRIES: u64 = 512;

/// A chunk never ends before this many entries, so a run of unlucky hashes cannot produce a
/// chunk of one key whose addressing overhead exceeds its payload.
const MIN_CHUNK_ENTRIES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub key: String,
    pub language: String,
    pub entries: usize,
    pub bytes: u64,
}

/// One revision's localization expressed as chunk references.
#[derive(Debug, Clone, Default)]
pub struct Chunked {
    pub chunks: Vec<Chunk>,
}

impl Chunked {
    pub fn referenced_bytes(&self) -> u64 {
        self.chunks.iter().map(|chunk| chunk.bytes).sum()
    }

    pub fn keys(&self) -> BTreeSet<&str> {
        self.chunks.iter().map(|chunk| chunk.key.as_str()).collect()
    }
}

/// One chunk per language table.
pub fn whole_language(localization: &Localization) -> Chunked {
    let mut chunks = Vec::new();
    for (language, table) in &localization.languages {
        let payload = encode(table.entries.iter());
        chunks.push(Chunk {
            key: sha256(payload.as_bytes()),
            language: language.clone(),
            entries: table.entries.len(),
            bytes: payload.len() as u64,
        });
    }
    Chunked { chunks }
}

/// Boundaries chosen by content, so an inserted key dirties one chunk rather than all of them.
pub fn content_defined(localization: &Localization) -> Chunked {
    let mut chunks = Vec::new();
    for (language, table) in &localization.languages {
        let mut pending: Vec<(&String, &String)> = Vec::new();
        for (key, value) in &table.entries {
            pending.push((key, value));
            if pending.len() >= MIN_CHUNK_ENTRIES && is_boundary(key) {
                chunks.push(seal(language, &pending));
                pending.clear();
            }
        }
        if !pending.is_empty() {
            chunks.push(seal(language, &pending));
        }
    }
    Chunked { chunks }
}

/// A boundary is a property of the key's own bytes, never of its position.
///
/// Position-based boundaries are what make fixed-size chunking useless here: insert one key
/// near the front of a sorted table and every subsequent boundary moves, so every chunk after
/// it gets a new content address despite holding the same keys.
fn is_boundary(key: &str) -> bool {
    let digest = sha256(key.as_bytes());
    let leading = u64::from_str_radix(&digest[..8], 16).unwrap_or(0);
    leading % TARGET_CHUNK_ENTRIES == 0
}

fn seal(language: &str, entries: &[(&String, &String)]) -> Chunk {
    let payload = encode(entries.iter().map(|(key, value)| (*key, *value)));
    Chunk {
        key: sha256(payload.as_bytes()),
        language: language.to_owned(),
        entries: entries.len(),
        bytes: payload.len() as u64,
    }
}

/// The canonical chunk payload: `key\0value\n`, in key order.
///
/// Not JSON. The chunk is addressed by its content, so its encoding is part of its identity,
/// and a serializer whose member order or escaping could change between versions would change
/// every address in the store without any localization changing.
fn encode<'a>(entries: impl Iterator<Item = (&'a String, &'a String)>) -> String {
    let mut payload = String::new();
    for (key, value) in entries {
        payload.push_str(key);
        payload.push('\0');
        payload.push_str(value);
        payload.push('\n');
    }
    payload
}

/// What a shared store across several revisions would cost and recover.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreMeasurement {
    pub scheme: String,
    /// Bytes if every revision carried its own copy.
    pub per_revision_total_bytes: u64,
    /// Bytes actually stored once deduplicated.
    pub unique_bytes: u64,
    pub unique_chunks: usize,
    pub total_chunk_references: usize,
    /// `per_revision_total_bytes - unique_bytes`.
    pub recovered_bytes: u64,
    pub deduplication_ratio: f64,
    /// Per revision, so a reader can see the cost is not evenly spread.
    pub per_revision: BTreeMap<String, u64>,
}

pub fn measure(scheme: &str, revisions: &BTreeMap<String, Chunked>) -> StoreMeasurement {
    let mut unique: BTreeMap<&str, u64> = BTreeMap::new();
    let mut per_revision = BTreeMap::new();
    let mut per_revision_total_bytes = 0u64;
    let mut references = 0usize;

    for (revision, chunked) in revisions {
        let referenced = chunked.referenced_bytes();
        per_revision_total_bytes += referenced;
        per_revision.insert(revision.clone(), referenced);
        references += chunked.chunks.len();
        for chunk in &chunked.chunks {
            unique.insert(chunk.key.as_str(), chunk.bytes);
        }
    }

    let unique_bytes: u64 = unique.values().sum();
    StoreMeasurement {
        scheme: scheme.to_owned(),
        per_revision_total_bytes,
        unique_bytes,
        unique_chunks: unique.len(),
        total_chunk_references: references,
        recovered_bytes: per_revision_total_bytes.saturating_sub(unique_bytes),
        deduplication_ratio: if unique_bytes == 0 {
            0.0
        } else {
            per_revision_total_bytes as f64 / unique_bytes as f64
        },
        per_revision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localization::Language;

    fn table(prefix: &str, count: usize) -> Localization {
        let mut entries = BTreeMap::new();
        for index in 0..count {
            entries.insert(
                format!("{prefix}_key_{index:06}"),
                format!("value for {index}"),
            );
        }
        let mut localization = Localization::default();
        localization
            .languages
            .insert("english".into(), Language { entries, shadowed: 0 });
        localization
    }

    #[test]
    fn an_inserted_key_dirties_one_content_defined_chunk_rather_than_all_of_them() {
        let base = table("a", 4_000);
        let mut extended = base.clone();
        // Insert near the FRONT of the sorted key space. This is the case that defeats
        // fixed-size chunking: every later boundary would move.
        extended
            .languages
            .get_mut("english")
            .unwrap()
            .entries
            .insert("a_key_000000_inserted".into(), "new".into());

        let base_chunks = content_defined(&base);
        let extended_chunks = content_defined(&extended);
        let before = base_chunks.keys();
        let after = extended_chunks.keys();

        let shared = before.intersection(&after).count();
        let dirty = after.difference(&before).count();

        assert!(shared > 0, "chunking must recover something");
        assert!(
            dirty <= 2,
            "one insertion dirtied {dirty} chunks of {} — the boundary is position-dependent",
            after.len()
        );

        // The negative control for the claim: whole-language chunking recovers nothing at all
        // from the same edit, because its one chunk covers the inserted key.
        let whole_base = whole_language(&base);
        let whole_extended = whole_language(&extended);
        assert_eq!(
            whole_base.keys().intersection(&whole_extended.keys()).count(),
            0
        );
    }

    #[test]
    fn deduplication_across_identical_revisions_is_exact() {
        let shared = content_defined(&table("a", 2_000));
        let revisions = BTreeMap::from([
            ("one".to_owned(), shared.clone()),
            ("two".to_owned(), shared.clone()),
            ("three".to_owned(), shared.clone()),
        ]);

        let measured = measure("content_defined", &revisions);
        assert_eq!(measured.unique_bytes, shared.referenced_bytes());
        assert_eq!(measured.per_revision_total_bytes, shared.referenced_bytes() * 3);
        assert!((measured.deduplication_ratio - 3.0).abs() < 1e-9);
    }

    #[test]
    fn no_chunk_is_smaller_than_the_floor_except_the_last() {
        let chunked = content_defined(&table("a", 5_000));
        assert!(chunked.chunks.len() > 1, "the fixture must produce several chunks");
        for chunk in &chunked.chunks[..chunked.chunks.len() - 1] {
            assert!(chunk.entries >= MIN_CHUNK_ENTRIES, "{chunk:?}");
        }
    }
}
