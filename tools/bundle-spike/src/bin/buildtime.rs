//! `b1-build` — how long does a complete build take, where does the time go, and what does
//! the shared Asset Store cost?
//!
//! The deciding measurement for `docs/technical-design.md:152`: an awaited asynchronous Tauri
//! command is preferred when p95 complete builds for every representative Target Mod are at
//! most three seconds, and an explicit host-owned job is required otherwise.
//!
//! Assets are measured here rather than in a record of their own because materialization *is*
//! a build phase. Splitting it out would have meant converting the same icons twice to answer
//! one question about how long a build takes and another about what the store holds.
//!
//! Two artifacts, and the split is the point: `assets.json` and `phases.json` hold counts,
//! byte totals, and keys and are byte-compared by the drift gate; `timings.json` holds
//! distributions and is not.

use bundle_spike::bundle::{Layout, LocalizationPlacement, SearchScope, Shape};
use bundle_spike::corpus::{self, CorpusIdentity};
use bundle_spike::record::{self, Artifact};
use bundle_spike::{assets, pipeline, timing};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

const PURPOSE: &str = "End-to-end and per-phase time for a complete build of every revision \
    case, cold and warm, plus what the shared content-addressed Asset Store holds and \
    deduplicates. Decides the build invocation model against the declared three-second p95 \
    threshold. Phase composition, asset keys, and byte totals are byte-compared by the drift \
    gate; the timing distributions beside them are judged against the declared budgets and \
    are deliberately not compared against a prior capture.";

/// The shape the size measurement selected. Timing a shape the size budget already rejected
/// would answer a question nobody is going to ask.
const SHAPE: Shape = Shape {
    layout: Layout::PerDocument,
    localization: LocalizationPlacement::ClosureInBundle,
    search: SearchScope::SelectedAndEnglish,
    selected_language: "english",
};

#[derive(Serialize)]
struct PhaseRow {
    case: String,
    /// Phase share of the first (cold) build, as a percentage of its total.
    cold_share_percent: BTreeMap<String, f64>,
    revision: String,
    entries: usize,
    bundle_files: usize,
    bundle_bytes: u64,
    required_asset_keys: usize,
    required_localization_chunks: usize,
}

#[derive(Serialize)]
struct AssetRow {
    case: String,
    requested: usize,
    converted: usize,
    reused: usize,
    failed: usize,
    source_bytes: u64,
    output_bytes: u64,
    distinct_keys: usize,
}

#[derive(Serialize)]
struct StoreRow {
    unique_blobs: usize,
    unique_bytes: u64,
    /// Sum of every revision's referenced blob bytes, as if nothing were shared.
    referenced_bytes_across_revisions: u64,
    deduplication_ratio: f64,
}

#[derive(Serialize)]
struct Timings {
    shape: String,
    iterations: usize,
    warmup: usize,
    build_budget_ms: f64,
    /// Per case: the complete build, repeated. The first of each set is discarded as warm-up,
    /// so these are warm builds against a populated Asset Store.
    complete_build: Vec<timing::Distribution>,
    /// One cold build per case, in this process, before the store was populated.
    cold_build_ms: BTreeMap<String, f64>,
    cold_phases: BTreeMap<String, bundle_spike::pipeline::Phases>,
    meets_awaited_command_budget: bool,
}

fn main() -> std::io::Result<()> {
    let capture = std::env::args().any(|argument| argument == "--capture");

    let revisions_root = pipeline::revisions_root();
    let store_root = pipeline::asset_store_root();
    let _ = std::fs::remove_dir_all(&revisions_root);
    let _ = std::fs::remove_dir_all(&store_root);

    let mut store = assets::Store::open(&store_root)?;
    let mut corpora: BTreeMap<String, CorpusIdentity> = BTreeMap::new();
    let mut phase_rows = Vec::new();
    let mut asset_rows = Vec::new();
    let mut distributions = Vec::new();
    let mut cold_build_ms = BTreeMap::new();
    let mut cold_phases = BTreeMap::new();
    let mut referenced_across_revisions = 0u64;
    let mut warnings = Vec::new();

    for case in corpus::default_cases() {
        eprintln!("build: {}", case.id);
        let snapshots = pipeline::snapshots(&case)?;
        for contributor in case.contributors() {
            if !corpora.contains_key(&contributor.id) {
                corpora.insert(
                    contributor.id.clone(),
                    corpus::identify(contributor, &snapshots[&contributor.id])?,
                );
            }
        }

        // The cold build: nothing in the Asset Store yet for icons this case is the first to
        // reference, and no bundle on disk. Recorded once and separately, because a cold
        // conversion cannot be repeated 30 times without becoming a warm one.
        let cold = pipeline::build(&case, &snapshots, SHAPE, &mut store, &revisions_root)?;
        cold_build_ms.insert(case.id.clone(), cold.phases.total_ms());
        cold_phases.insert(case.id.clone(), cold.phases.clone());

        let mut asset_stats = assets::Stats::default();
        let mut distinct = BTreeSet::new();

        // Outside the timer: remove the bundle the previous iteration published. Retention
        // cleanup deletes a superseded bundle after its last handle closes; publication does
        // not, and a loop that republishes onto its own destination would otherwise charge
        // every build for deleting the last one.
        let published_path = built_path(&revisions_root, &cold.revision);
        let distribution = timing::measure(
            format!("complete_build:{}", case.id),
            || {
                let _ = std::fs::remove_dir_all(&published_path);
            },
            || pipeline::build(&case, &snapshots, SHAPE, &mut store, &revisions_root),
            |result| {
                // Observed rather than discarded: a build whose result is never looked at can
                // be optimized away, and a benchmark of nothing is very fast.
                let built = result.as_ref().expect("build succeeds");
                assert!(!built.revision.is_empty());
                asset_stats = built.asset_stats.clone();
                distinct = built.asset_keys.iter().cloned().collect();
            },
        );

        referenced_across_revisions += asset_stats.output_bytes;

        let total = cold.phases.total_ms().max(0.01);
        phase_rows.push(PhaseRow {
            case: case.id.clone(),
            cold_share_percent: BTreeMap::from([
                ("fingerprint".into(), share(cold.phases.fingerprint_ms, total)),
                ("resolve".into(), share(cold.phases.resolve_ms, total)),
                ("generate".into(), share(cold.phases.generate_ms, total)),
                ("assets".into(), share(cold.phases.assets_ms, total)),
                ("localization_chunk".into(), share(cold.phases.localization_chunk_ms, total)),
                ("write".into(), share(cold.phases.write_ms, total)),
                ("validate".into(), share(cold.phases.validate_ms, total)),
                ("reverify".into(), share(cold.phases.reverify_ms, total)),
                ("publish".into(), share(cold.phases.publish_ms, total)),
            ]),
            revision: cold.revision.clone(),
            entries: cold.documentation.entries.len(),
            bundle_files: cold.written.file_count,
            bundle_bytes: cold.written.total_bytes,
            required_asset_keys: cold.asset_keys.len(),
            required_localization_chunks: cold.localization_chunks.len(),
        });

        asset_rows.push(AssetRow {
            case: case.id.clone(),
            requested: asset_stats.requested,
            converted: asset_stats.converted,
            reused: asset_stats.reused,
            failed: asset_stats.failed,
            source_bytes: asset_stats.source_bytes,
            output_bytes: asset_stats.output_bytes,
            distinct_keys: distinct.len(),
        });

        if !distribution.meets(timing::budget::COMPLETE_BUILD_MS) {
            warnings.push(format!(
                "{}: p95 complete build {:.0} ms exceeds the {:.0} ms awaited-command \
                 threshold, so this case requires an explicit host-owned job",
                case.id,
                distribution.p95_ms,
                timing::budget::COMPLETE_BUILD_MS
            ));
        }
        distributions.push(distribution);
    }

    let blobs = store.blobs()?;
    let unique_bytes: u64 = blobs.values().sum();
    let store_row = StoreRow {
        unique_blobs: blobs.len(),
        unique_bytes,
        referenced_bytes_across_revisions: referenced_across_revisions,
        deduplication_ratio: if unique_bytes == 0 {
            0.0
        } else {
            (referenced_across_revisions as f64 / unique_bytes as f64 * 1000.0).round() / 1000.0
        },
    };

    let timings = Timings {
        shape: SHAPE.id(),
        iterations: timing::MEASURED_ITERATIONS,
        warmup: timing::WARMUP_ITERATIONS,
        build_budget_ms: timing::budget::COMPLETE_BUILD_MS,
        meets_awaited_command_budget: distributions
            .iter()
            .all(|d| d.meets(timing::budget::COMPLETE_BUILD_MS)),
        complete_build: distributions,
        cold_build_ms,
        cold_phases,
    };

    let summary = render(&phase_rows, &asset_rows, &store_row, &timings);
    print!("{summary}");

    if capture {
        let directory = record::write(
            "b1-build",
            PURPOSE,
            corpora.into_values().collect(),
            vec![
                Artifact::identity("phases.json", record::to_json(&phase_rows)),
                Artifact::identity("assets.json", record::to_json(&asset_rows)),
                Artifact::identity("asset-store.json", record::to_json(&store_row)),
                Artifact::timings("timings.json", record::to_json(&timings)),
                Artifact::timings("summary.txt", summary),
            ],
            warnings,
        )?;
        eprintln!("captured {}", directory.display());
    }
    Ok(())
}

fn built_path(revisions_root: &std::path::Path, revision: &str) -> std::path::PathBuf {
    bundle_spike::bundle::published_path(revisions_root, revision)
}

fn share(part: f64, total: f64) -> f64 {
    ((part / total) * 1000.0).round() / 10.0
}

fn render(
    phases: &[PhaseRow],
    assets: &[AssetRow],
    store: &StoreRow,
    timings: &Timings,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# b1-build  shape {}\n", timings.shape);

    let _ = writeln!(
        out,
        "{:<14} {:>10} {:>10} {:>10} {:>10} {:>8}",
        "case", "cold ms", "median", "p95", "max", "budget"
    );
    for distribution in &timings.complete_build {
        let case = distribution
            .label
            .rsplit(':')
            .next()
            .unwrap_or(&distribution.label);
        let _ = writeln!(
            out,
            "{:<14} {:>10.0} {:>10.1} {:>10.1} {:>10.1} {:>8}",
            case,
            timings.cold_build_ms.get(case).copied().unwrap_or(0.0),
            distribution.median_ms,
            distribution.p95_ms,
            distribution.max_ms,
            if distribution.meets(timings.build_budget_ms) { "ok" } else { "MISS" },
        );
    }

    let _ = writeln!(out, "\ncold phase share, percent of that build's total");
    let _ = writeln!(
        out,
        "{:<14} {:>7} {:>8} {:>9} {:>7} {:>8} {:>6} {:>9} {:>9} {:>8}",
        "case", "finger", "resolve", "generate", "assets", "loc-chunk", "write", "validate", "reverify", "publish"
    );
    for row in phases {
        let get = |name: &str| row.cold_share_percent.get(name).copied().unwrap_or(0.0);
        let _ = writeln!(
            out,
            "{:<14} {:>6.1}% {:>7.1}% {:>8.1}% {:>6.1}% {:>7.1}% {:>5.1}% {:>8.1}% {:>8.1}% {:>7.1}%",
            row.case,
            get("fingerprint"),
            get("resolve"),
            get("generate"),
            get("assets"),
            get("localization_chunk"),
            get("write"),
            get("validate"),
            get("reverify"),
            get("publish"),
        );
    }

    let _ = writeln!(
        out,
        "\n{:<14} {:>10} {:>10} {:>8} {:>8} {:>12} {:>12}",
        "case", "requested", "converted", "reused", "failed", "source", "output"
    );
    for row in assets {
        let _ = writeln!(
            out,
            "{:<14} {:>10} {:>10} {:>8} {:>8} {:>11.1}M {:>11.1}M",
            row.case,
            row.requested,
            row.converted,
            row.reused,
            row.failed,
            mib(row.source_bytes),
            mib(row.output_bytes),
        );
    }

    let _ = writeln!(
        out,
        "\nasset store: {} unique blobs, {:.1}M unique, {:.1}M referenced across revisions, {:.2}x",
        store.unique_blobs,
        mib(store.unique_bytes),
        mib(store.referenced_bytes_across_revisions),
        store.deduplication_ratio,
    );
    let _ = writeln!(
        out,
        "\nawaited Tauri command budget ({:.0} ms p95): {}",
        timings.build_budget_ms,
        if timings.meets_awaited_command_budget { "met" } else { "MISSED" }
    );
    out
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
