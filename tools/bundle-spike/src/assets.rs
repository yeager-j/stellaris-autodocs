//! The shared content-addressed Asset Store.
//!
//! Conversion itself is not re-litigated here. `ADR 0008` accepted `image_dds` behind a pinned
//! recipe, and this module reaches it through `dds_spike` rather than reimplementing it, so
//! the bytes measured are the bytes that decision produced. What is measured here is the
//! *store*: unique bytes against per-revision referenced bytes, the deduplication ratio across
//! revisions that share Vanilla icons, and cold conversion against warm reuse.
//!
//! One key per source-bytes-plus-recipe pair (`docs/technical-design.md:503`). Paths, mod
//! titles, declared versions, and filesystem timestamps do not participate, which is exactly
//! why the same vanilla icon referenced by four revisions is stored once.
//!
//! Publication is atomic per blob: write to a temporary name beside the target and rename. A
//! reader that found a half-written PNG at a content address would have found a blob whose
//! content does not match its address, which is the one thing a content-addressed store
//! promises cannot happen.

use crate::digest::sha256;
use dds_spike::model::Outcome;
use dds_spike::recipe::{OutputFormat, Recipe};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// One slot's typed materialization outcome.
///
/// Mirrors `docs/technical-design.md:263`: exactly one outcome per requested slot, either a
/// key plus trusted metadata or a typed failure. `analysis::finalize` — here, the caller —
/// turns a failure into a placeholder and a scoped issue rather than into a required key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Materialized {
    Blob {
        key: String,
        source_bytes: u64,
        output_bytes: u64,
        /// True when an existing blob satisfied the request without decoding.
        reused: bool,
    },
    MissingBytes { detail: String },
    MalformedMedia { detail: String },
    UnsupportedFormat { detail: String },
    ConversionFailure { detail: String },
}

impl Materialized {
    pub fn kind(&self) -> &'static str {
        match self {
            Materialized::Blob { .. } => "blob",
            Materialized::MissingBytes { .. } => "missing-bytes",
            Materialized::MalformedMedia { .. } => "malformed-media",
            Materialized::UnsupportedFormat { .. } => "unsupported-format",
            Materialized::ConversionFailure { .. } => "conversion-failure",
        }
    }

    pub fn key(&self) -> Option<&str> {
        match self {
            Materialized::Blob { key, .. } => Some(key),
            _ => None,
        }
    }
}

pub struct Store {
    root: PathBuf,
    recipe: Recipe,
    /// Trusted metadata for blobs this process has already published or validated.
    ///
    /// A cache of proof, not of content. `docs/technical-design.md:525` permits reusing a blob
    /// only when trusted metadata or content validation proves it matches the key; path
    /// existence alone is explicitly insufficient, because a truncated file exists.
    known: BTreeMap<String, u64>,
    /// When each key was last handed to a caller.
    ///
    /// Runtime cleanup may remove unreferenced blobs, but only after a conservative grace
    /// period, so an asset URL issued shortly before a publication cannot race its deletion
    /// (`docs/technical-design.md:531`). Startup has no such process-lifetime history and
    /// deliberately sweeps everything — which is why the grace period lives on the store
    /// rather than in the cleanup call.
    issued: BTreeMap<String, Instant>,
}

/// How long a recently issued key is protected from runtime cleanup.
pub const RUNTIME_GRACE: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub requested: usize,
    pub converted: usize,
    pub reused: usize,
    pub failed: usize,
    pub source_bytes: u64,
    pub output_bytes: u64,
}

impl Store {
    pub fn open(root: impl Into<PathBuf>) -> std::io::Result<Store> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Store {
            root,
            recipe: Recipe::pinned(OutputFormat::Png),
            known: BTreeMap::new(),
            issued: BTreeMap::new(),
        })
    }

    pub fn recipe(&self) -> &Recipe {
        &self.recipe
    }

    /// Materialize one slot from a source asset path.
    pub fn materialize(&mut self, source: &Path, stats: &mut Stats) -> Materialized {
        stats.requested += 1;

        let Ok(bytes) = std::fs::read(source) else {
            stats.failed += 1;
            return Materialized::MissingBytes {
                detail: format!("{} produced no bytes", source.display()),
            };
        };
        stats.source_bytes += bytes.len() as u64;

        let key = dds_spike::recipe::asset_key(&bytes, &self.recipe);
        if let Some(output_bytes) = self.validated(&key) {
            stats.reused += 1;
            stats.output_bytes += output_bytes;
            self.issued.insert(key.clone(), Instant::now());
            return Materialized::Blob {
                key,
                source_bytes: bytes.len() as u64,
                output_bytes,
                reused: true,
            };
        }

        let image = match dds_spike::decode_a::adapt(&bytes, &self.recipe) {
            Outcome::Decoded(image) => image,
            Outcome::MissingBytes { detail } => {
                stats.failed += 1;
                return Materialized::MissingBytes { detail };
            }
            Outcome::MalformedMedia { detail } => {
                stats.failed += 1;
                return Materialized::MalformedMedia { detail };
            }
            Outcome::UnsupportedFormat { detail } => {
                stats.failed += 1;
                return Materialized::UnsupportedFormat { detail };
            }
            Outcome::ConversionFailure { detail } => {
                stats.failed += 1;
                return Materialized::ConversionFailure { detail };
            }
        };

        let encoded = match dds_spike::encode::encode_png(&image) {
            Ok(encoded) => encoded,
            Err(error) => {
                stats.failed += 1;
                return Materialized::ConversionFailure {
                    detail: error.to_string(),
                };
            }
        };

        if let Err(error) = self.publish(&key, &encoded) {
            stats.failed += 1;
            return Materialized::ConversionFailure {
                detail: format!("staging write failed: {error}"),
            };
        }

        stats.converted += 1;
        stats.output_bytes += encoded.len() as u64;
        self.known.insert(key.clone(), encoded.len() as u64);
        self.issued.insert(key.clone(), Instant::now());
        Materialized::Blob {
            key,
            source_bytes: bytes.len() as u64,
            output_bytes: encoded.len() as u64,
            reused: false,
        }
    }

    /// Whether a blob for this key exists and its content actually hashes to its address.
    fn validated(&mut self, key: &str) -> Option<u64> {
        if let Some(bytes) = self.known.get(key) {
            return Some(*bytes);
        }
        let path = self.blob_path(key);
        let bytes = std::fs::read(&path).ok()?;
        // The content check, not a path check. Re-deriving the key from the *source* is not
        // possible here — the store holds outputs — so the store records the output digest
        // alongside the blob and compares that.
        let recorded = std::fs::read_to_string(path.with_extension("sha256")).ok()?;
        if recorded.trim() != sha256(&bytes) {
            return None;
        }
        self.known.insert(key.to_owned(), bytes.len() as u64);
        Some(bytes.len() as u64)
    }

    fn publish(&self, key: &str, bytes: &[u8]) -> std::io::Result<()> {
        let path = self.blob_path(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let staging = path.with_extension("png.partial");
        std::fs::write(&staging, bytes)?;
        std::fs::write(path.with_extension("sha256"), sha256(bytes))?;
        std::fs::rename(&staging, &path)
    }

    /// Two levels of fan-out, so a store holding tens of thousands of blobs does not put them
    /// all in one directory.
    fn blob_path(&self, key: &str) -> PathBuf {
        self.root.join(&key[..2]).join(&key[2..4]).join(format!("{key}.png"))
    }

    /// Every blob the store currently holds.
    pub fn blobs(&self) -> std::io::Result<BTreeMap<String, u64>> {
        let mut found = BTreeMap::new();
        let mut stack = vec![self.root.clone()];
        while let Some(directory) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                match entry.file_type() {
                    Ok(kind) if kind.is_dir() => stack.push(path),
                    Ok(_) if path.extension().is_some_and(|e| e == "png") => {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                            found.insert(stem.to_owned(), size);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(found)
    }

    /// Delete every blob no retained manifest references.
    ///
    /// Fails conservatively and says so: the caller passes the live set derived from *every*
    /// readable retained manifest, and if any manifest could not be read the caller must not
    /// call this at all. `docs/technical-design.md:531` makes an unreadable manifest suppress
    /// deletion entirely, because an incomplete live set makes every unreferenced blob a
    /// possible false positive.
    pub fn collect(&mut self, live: &BTreeSet<String>) -> std::io::Result<Collected> {
        self.collect_with_grace(live, None)
    }

    /// Runtime cleanup: as [`Self::collect`], but a key issued within `grace` is retained even
    /// when no manifest references it.
    pub fn collect_with_grace(
        &mut self,
        live: &BTreeSet<String>,
        grace: Option<Duration>,
    ) -> std::io::Result<Collected> {
        let mut collected = Collected::default();
        for (key, size) in self.blobs()? {
            if live.contains(&key) {
                collected.retained += 1;
                collected.retained_bytes += size;
                continue;
            }
            if let Some(grace) = grace {
                if self.issued.get(&key).is_some_and(|at| at.elapsed() < grace) {
                    collected.retained += 1;
                    collected.retained_bytes += size;
                    collected.retained_by_grace += 1;
                    continue;
                }
            }
            let path = self.blob_path(&key);
            std::fs::remove_file(&path)?;
            let _ = std::fs::remove_file(path.with_extension("sha256"));
            self.known.remove(&key);
            collected.removed += 1;
            collected.removed_bytes += size;
        }
        Ok(collected)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collected {
    pub retained: usize,
    pub retained_bytes: u64,
    /// Retained only because the grace period protected them. Counted separately so a
    /// startup sweep and a runtime sweep cannot be confused for one another in a record.
    pub retained_by_grace: usize,
    pub removed: usize,
    pub removed_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bundle-spike-assets-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn fixture(name: &str) -> PathBuf {
        crate::corpus::repo_root()
            .join("fixtures/assets/dds/valid")
            .join(name)
    }

    #[test]
    fn the_same_source_bytes_convert_once_and_are_reused_thereafter() {
        let mut store = Store::open(scratch("reuse")).expect("store opens");
        let mut stats = Stats::default();
        let source = fixture("bgra8_2x2.dds");

        let first = store.materialize(&source, &mut stats);
        let second = store.materialize(&source, &mut stats);

        let (Materialized::Blob { key: first_key, reused: false, .. }, Materialized::Blob { key: second_key, reused: true, .. }) =
            (&first, &second)
        else {
            panic!("expected a conversion then a reuse: {first:?} {second:?}");
        };
        assert_eq!(first_key, second_key);
        assert_eq!(stats.converted, 1);
        assert_eq!(stats.reused, 1);
    }

    #[test]
    fn a_changed_source_byte_changes_the_key_and_an_unrelated_asset_does_not() {
        let mut store = Store::open(scratch("keys")).expect("store opens");
        let mut stats = Stats::default();

        let original = store.materialize(&fixture("bgra8_2x2.dds"), &mut stats);
        let other = store.materialize(&fixture("dxt1_opaque_4x4.dds"), &mut stats);

        let scratch_root = scratch("keys-input");
        std::fs::create_dir_all(&scratch_root).expect("scratch");
        let altered_path = scratch_root.join("altered.dds");
        let mut altered = std::fs::read(fixture("bgra8_2x2.dds")).expect("fixture reads");
        let last = altered.len() - 1;
        altered[last] ^= 0xff;
        std::fs::write(&altered_path, &altered).expect("write");
        let altered = store.materialize(&altered_path, &mut stats);

        assert_ne!(original.key(), altered.key(), "a source byte change must change the key");
        assert_ne!(original.key(), other.key());
        assert!(original.key().is_some() && altered.key().is_some());
    }

    #[test]
    fn garbage_collection_keeps_live_blobs_and_removes_the_rest() {
        let mut store = Store::open(scratch("gc")).expect("store opens");
        let mut stats = Stats::default();

        let live = store.materialize(&fixture("bgra8_2x2.dds"), &mut stats);
        let dead = store.materialize(&fixture("dxt1_opaque_4x4.dds"), &mut stats);

        let live_set: BTreeSet<String> =
            BTreeSet::from([live.key().expect("live key").to_owned()]);
        let collected = store.collect(&live_set).expect("collect");

        assert_eq!(collected.retained, 1);
        assert_eq!(collected.removed, 1);
        let remaining = store.blobs().expect("blobs");
        assert!(remaining.contains_key(live.key().unwrap()));
        assert!(!remaining.contains_key(dead.key().unwrap()));
    }

    #[test]
    fn a_blob_whose_content_does_not_match_its_recorded_digest_is_not_reused() {
        let mut store = Store::open(scratch("corrupt")).expect("store opens");
        let mut stats = Stats::default();
        let source = fixture("bgra8_2x2.dds");

        let first = store.materialize(&source, &mut stats);
        let key = first.key().expect("key").to_owned();

        // Corrupt the blob and drop the in-process proof, so reuse must fall back to the
        // content check rather than to the fact that a file exists at the path.
        let path = store.blob_path(&key);
        std::fs::write(&path, b"not a png").expect("corrupt");
        store.known.remove(&key);

        assert!(store.validated(&key).is_none(), "path existence is not proof");

        let second = store.materialize(&source, &mut stats);
        assert!(
            matches!(second, Materialized::Blob { reused: false, .. }),
            "a corrupt blob must be reconverted, not trusted: {second:?}"
        );
    }
}
