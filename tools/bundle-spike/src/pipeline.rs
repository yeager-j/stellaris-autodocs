//! The complete build, phase by phase.
//!
//! One function, because the thing being timed is the *complete* build and a harness that
//! measured stages in isolation would miss what the technical design is actually waiting on:
//! whether a build is short enough to be an awaited asynchronous Tauri command, or long enough
//! to need an explicit host-owned job with reconnectable status
//! (`docs/technical-design.md:152`). That question is about the sum, and the per-phase split
//! only exists to say *which* phase decided it.
//!
//! The phases follow `docs/technical-design.md:414` rather than a convenient ordering:
//! fingerprint the complete logical snapshot, parse and resolve from those exact bytes,
//! generate, materialize the assets the draft requested, write, validate, and only then
//! publish. The fingerprint is computed twice on purpose — once to establish the snapshot and
//! once immediately before publication — because that is what proves the source did not change
//! during analysis, and skipping the second pass would make every timing here optimistic about
//! a protocol the product cannot skip.

use crate::assets::{self, Materialized};
use crate::bundle::{self, Shape, Written};
use crate::corpus::{RevisionCase, Snapshot};
use crate::digest::{sha256, Stream};
use crate::docmodel::{AssetSlot, Documentation};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Phases {
    pub fingerprint_ms: f64,
    pub resolve_ms: f64,
    pub generate_ms: f64,
    pub assets_ms: f64,
    pub localization_chunk_ms: f64,
    pub write_ms: f64,
    pub validate_ms: f64,
    pub reverify_ms: f64,
    pub publish_ms: f64,
}

impl Phases {
    pub fn total_ms(&self) -> f64 {
        self.fingerprint_ms
            + self.resolve_ms
            + self.generate_ms
            + self.assets_ms
            + self.localization_chunk_ms
            + self.write_ms
            + self.validate_ms
            + self.reverify_ms
            + self.publish_ms
    }
}

pub struct Build {
    pub documentation: Documentation,
    pub written: Written,
    pub published: PathBuf,
    pub revision: String,
    pub phases: Phases,
    pub asset_stats: assets::Stats,
    pub asset_keys: Vec<String>,
    pub localization_chunks: Vec<String>,
    pub input_fingerprint: String,
}

/// Where bundles and the shared stores live during a run.
///
/// Environment-overridable like every other root in this harness, and defaulting outside the
/// repository: a bundle is hundreds of megabytes and no corpus content or generated artifact
/// belongs in version control.
pub fn work_root() -> PathBuf {
    match std::env::var("BUNDLE_SPIKE_WORK") {
        Ok(value) if !value.is_empty() => PathBuf::from(value),
        _ => std::env::temp_dir().join("bundle-spike-work"),
    }
}

pub fn revisions_root() -> PathBuf {
    work_root().join("revisions")
}

pub fn asset_store_root() -> PathBuf {
    work_root().join("asset-store")
}

pub fn snapshots(case: &RevisionCase) -> std::io::Result<BTreeMap<String, Snapshot>> {
    let mut snapshots = BTreeMap::new();
    for contributor in case.contributors() {
        snapshots.insert(contributor.id.clone(), crate::corpus::snapshot(contributor)?);
    }
    Ok(snapshots)
}

/// Hash every file the snapshot presents, in normalized logical-path order.
///
/// Whole-content hashing rather than a timestamp comparison. `docs/technical-design.md:428`
/// permits cached metadata to avoid work only when the authoritative fingerprint protocol
/// still proves the same content identity, and the point of measuring it here is to know what
/// the correctness-first version costs before anyone decides to optimize it.
pub fn fingerprint(
    snapshots: &BTreeMap<String, Snapshot>,
    referenced_assets: &BTreeMap<String, PathBuf>,
) -> std::io::Result<String> {
    let mut stream = Stream::new();
    for (id, snapshot) in snapshots {
        for file in snapshot.script.iter().chain(snapshot.localisation.iter()) {
            let bytes = std::fs::read(&file.absolute)?;
            stream.push(&format!("{id}/{}", file.logical), &sha256(&bytes));
        }
    }
    // Referenced source assets participate in freshness identity, so an icon edit invalidates
    // the revision even when no script changed (`docs/technical-design.md:430`). Unrelated
    // binary assets are not hashed: the build does not pay for every texture in a mod merely
    // because it is present.
    for (slot, path) in referenced_assets {
        let bytes = std::fs::read(path)?;
        stream.push(&format!("asset/{slot}"), &sha256(&bytes));
    }
    Ok(stream.finish())
}

pub fn build(
    case: &RevisionCase,
    snapshots: &BTreeMap<String, Snapshot>,
    shape: Shape,
    store: &mut assets::Store,
    revisions_root: &Path,
) -> std::io::Result<Build> {
    std::fs::create_dir_all(revisions_root)?;
    let mut phases = Phases::default();

    // The first pass establishes the snapshot's identity. It cannot yet include referenced
    // assets, because which assets are referenced is not known until the draft exists.
    let started = Instant::now();
    let mut input_fingerprint = fingerprint(snapshots, &BTreeMap::new())?;
    phases.fingerprint_ms = millis(started.elapsed());

    let started = Instant::now();
    let resolved = crate::resolve::resolve(case, snapshots)?;
    phases.resolve_ms = millis(started.elapsed());

    let started = Instant::now();
    let scope = match shape.localization {
        bundle::LocalizationPlacement::ClosureInBundle => {
            crate::generate::LocalizationScope::CitedClosure
        }
        _ => crate::generate::LocalizationScope::AllKeys,
    };
    let mut documentation = crate::generate::generate(&resolved, &resolved.sources, scope);
    phases.generate_ms = millis(started.elapsed());

    // Asset materialization, and then the substitution `analysis::finalize` owns: a failed
    // slot becomes a deterministic placeholder and a scoped issue, never a required key.
    let started = Instant::now();
    let mut asset_stats = assets::Stats::default();
    let mut asset_keys = BTreeSet::new();
    let mut referenced_assets = BTreeMap::new();
    for entry in &mut documentation.entries {
        let AssetSlot::Resolved {
            contributor,
            logical,
            ..
        } = &entry.icon
        else {
            continue;
        };
        let Some(snapshot) = snapshots.get(contributor) else {
            entry.icon = AssetSlot::Placeholder {
                reason: format!("contributor {contributor} is not in this revision"),
            };
            continue;
        };

        let source = snapshot.root.join(logical);
        match store.materialize(&source, &mut asset_stats) {
            Materialized::Blob { key, .. } => {
                referenced_assets.insert(format!("{contributor}/{logical}"), source);
                asset_keys.insert(key.clone());
                entry.icon = AssetSlot::Resolved {
                    contributor: contributor.clone(),
                    logical: logical.clone(),
                    key: Some(key),
                };
            }
            failure => {
                entry.icon = AssetSlot::Placeholder {
                    reason: format!("{}: {failure:?}", failure.kind()),
                };
            }
        }
    }
    phases.assets_ms = millis(started.elapsed());

    // Timed, because it is not free and it was not free before it was timed. Content-defined
    // chunking hashes every localization key to find a boundary and then hashes every chunk
    // payload — roughly 1.5 million digests over 150 MiB per build. The first capture of
    // `b1-build` left this outside every phase, so the phase sum silently under-reported the
    // build it was supposed to account for and disagreed with the wall clock beside it.
    //
    // Only the shared-store arm pays it. A bundle carrying the cited closure has nothing to
    // chunk, which is most of why that arm is faster rather than merely smaller.
    let started = Instant::now();
    let localization_chunks: Vec<String> =
        if shape.localization == bundle::LocalizationPlacement::AllKeysSharedStore {
            crate::locstore::content_defined(&documentation.localization)
                .chunks
                .into_iter()
                .map(|chunk| chunk.key)
                .collect()
        } else {
            Vec::new()
        };
    phases.localization_chunk_ms = millis(started.elapsed());
    let asset_keys: Vec<String> = asset_keys.into_iter().collect();

    let started = Instant::now();
    let written = bundle::write(
        &documentation,
        shape,
        revisions_root,
        &asset_keys,
        &localization_chunks,
    )?;
    phases.write_ms = millis(started.elapsed());

    let started = Instant::now();
    let (_, validation) = bundle::validate(&written.root)?;
    phases.validate_ms = millis(started.elapsed());
    if !validation.valid() {
        return Err(std::io::Error::other(format!(
            "staged bundle failed its own validation: {validation:?}"
        )));
    }

    // The second fingerprint pass, now including every referenced source asset. A mismatch
    // means the source changed during analysis and the candidate is discarded without changing
    // the published revision.
    let started = Instant::now();
    let final_fingerprint = fingerprint(snapshots, &referenced_assets)?;
    phases.reverify_ms = millis(started.elapsed());
    input_fingerprint = sha256(format!("{input_fingerprint}\n{final_fingerprint}").as_bytes());

    let started = Instant::now();
    let published = bundle::publish(&written.root, revisions_root)?;
    phases.publish_ms = millis(started.elapsed());

    Ok(Build {
        revision: written.manifest.revision.clone(),
        documentation,
        written: Written {
            root: published.clone(),
            ..written
        },
        published,
        phases,
        asset_stats,
        asset_keys,
        localization_chunks,
        input_fingerprint,
    })
}

fn millis(duration: Duration) -> f64 {
    let value = duration.as_secs_f64() * 1000.0;
    (value * 100.0).round() / 100.0
}
