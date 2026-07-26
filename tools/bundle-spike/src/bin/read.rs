//! `b3-read` — how fast is a published revision to open, search, and read from, and how much
//! memory does an active index hold?
//!
//! Cold figures are measured in a **separate process**, which is what the declared definition
//! of cold requires and what a fresh `Reader` inside a warm process cannot honestly provide.
//! The child performs exactly one operation and reports only that operation's own elapsed
//! time, so process spawn cost stays out of the number while the cache state stays genuinely
//! cold. Neither figure claims to evict the operating system's filesystem cache.
//!
//! Retained memory is read twice: the model's own deep byte accounting, which knows exactly
//! what it retained and nothing about allocator behaviour, and max-RSS, which knows the
//! opposite. Reporting one alone would be reporting half the question.

use bundle_spike::bundle::{Layout, LocalizationPlacement, SearchScope, Shape};
use bundle_spike::corpus::{self, CorpusIdentity};
use bundle_spike::docmodel::EntryKey;
use bundle_spike::reader::Reader;
use bundle_spike::record::{self, Artifact};
use bundle_spike::{assets, pipeline, search, timing};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

const PURPOSE: &str = "Cold and warm latency for revision open through a validated Revision \
    Reader, host search at the maximum result limit, and a documentation-record read, plus \
    the memory an active browse and single-language search index retains. Cold figures are \
    measured in a separate process per iteration; warm figures reuse an open reader. Retained \
    memory is reported from two independent readings. Timing distributions are judged against \
    the budgets declared before capture and are not compared against a prior run.";

const SHAPE: Shape = Shape {
    layout: Layout::PerDocument,
    localization: LocalizationPlacement::ClosureInBundle,
    search: SearchScope::SelectedAndEnglish,
    selected_language: "english",
};

#[derive(Serialize)]
struct MemoryRow {
    case: String,
    entries: usize,
    search_entries: usize,
    /// The model's own accounting of what the decoded index holds.
    index_retained_bytes: u64,
    browse_retained_bytes: u64,
    /// Max-RSS delta across loading the index and browse data, in this process.
    rss_delta_bytes: u64,
    meets_budget: bool,
}

#[derive(Serialize)]
struct Timings {
    shape: String,
    iterations: usize,
    warmup: usize,
    budgets_ms: BTreeMap<String, f64>,
    distributions: Vec<timing::Distribution>,
    budget_outcomes: BTreeMap<String, bool>,
}

fn main() -> std::io::Result<()> {
    // Child mode: one cold operation, one number, then exit.
    let arguments: Vec<String> = std::env::args().collect();
    if let Some(position) = arguments.iter().position(|a| a == "--child") {
        return child(&arguments[position + 1], &arguments[position + 2], &arguments[position + 3]);
    }

    let capture = arguments.iter().any(|argument| argument == "--capture");
    let revisions_root = pipeline::work_root().join("read-revisions");
    let store_root = pipeline::asset_store_root();
    let _ = std::fs::remove_dir_all(&revisions_root);

    let mut store = assets::Store::open(&store_root)?;
    let mut corpora: BTreeMap<String, CorpusIdentity> = BTreeMap::new();
    let mut memory_rows = Vec::new();
    let mut distributions = Vec::new();
    let mut warnings = Vec::new();
    let mut budget_outcomes = BTreeMap::new();

    let executable = std::env::current_exe()?;

    for case in corpus::default_cases() {
        eprintln!("read: {}", case.id);
        let snapshots = pipeline::snapshots(&case)?;
        for contributor in case.contributors() {
            if !corpora.contains_key(&contributor.id) {
                corpora.insert(
                    contributor.id.clone(),
                    corpus::identify(contributor, &snapshots[&contributor.id])?,
                );
            }
        }

        let built = pipeline::build(&case, &snapshots, SHAPE, &mut store, &revisions_root)?;
        let published = vec![built.revision.clone()];
        let sample_keys: Vec<EntryKey> = built
            .documentation
            .entries
            .iter()
            .map(|entry| entry.key.clone())
            .collect();

        // Warm: one reader, reused.
        let mut reader = Reader::open_published(&revisions_root, &built.revision, &published)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let _ = reader.search_index("english")?;

        let mut cursor = 0usize;
        distributions.push(timing::measure(
            format!("warm_record_read:{}", case.id),
            || {},
            || {
                cursor = (cursor + 1) % sample_keys.len();
                reader.record(&sample_keys[cursor])
            },
            |result| {
                assert!(result.as_ref().expect("record read succeeds").is_some());
            },
        ));

        let index = reader.search_index("english")?.clone();
        distributions.push(timing::measure(
            format!("warm_search:{}", case.id),
            || {},
            || index.query("tech", &[], search::MAX_RESULT_LIMIT),
            |hits| assert!(!hits.is_empty(), "the corpus must produce hits for `tech`"),
        ));

        let browse_bytes: u64 = reader
            .browse("technology")?
            .iter()
            .map(|summary| record::to_compact_json(summary).len() as u64)
            .sum();

        let before = timing::max_rss_bytes();
        let held = reader.search_index("english")?.clone();
        let after = timing::max_rss_bytes();

        let retained = held.retained_bytes();
        let meets = retained + browse_bytes <= timing::budget::RETAINED_INDEX_BYTES;
        if !meets {
            warnings.push(format!(
                "{}: {} bytes retained for browse plus one language exceeds the declared \
                 {} byte budget",
                case.id,
                retained + browse_bytes,
                timing::budget::RETAINED_INDEX_BYTES
            ));
        }
        memory_rows.push(MemoryRow {
            case: case.id.clone(),
            entries: built.documentation.entries.len(),
            search_entries: held.entries.len(),
            index_retained_bytes: retained,
            browse_retained_bytes: browse_bytes,
            rss_delta_bytes: after.saturating_sub(before),
            meets_budget: meets,
        });

        // Cold: a new process per iteration.
        for operation in ["open", "search", "record"] {
            let samples = cold_samples(&executable, &revisions_root, &built.revision, operation)?;
            distributions.push(timing::Distribution::from_samples(
                format!("cold_{operation}:{}", case.id),
                timing::WARMUP_ITERATIONS,
                &samples,
            ));
        }

        reader.evict();
    }

    let budgets = BTreeMap::from([
        ("cold_open".to_owned(), timing::budget::COLD_REVISION_OPEN_MS),
        ("cold_search".to_owned(), timing::budget::COLD_SEARCH_MS),
        ("cold_record".to_owned(), timing::budget::COLD_RECORD_READ_MS),
        ("warm_search".to_owned(), timing::budget::WARM_SEARCH_MS),
        ("warm_record_read".to_owned(), timing::budget::WARM_RECORD_READ_MS),
        ("cold_open_validation".to_owned(), timing::budget::BUNDLE_VALIDATION_MS),
    ]);

    for distribution in &distributions {
        let (name, case) = distribution
            .label
            .split_once(':')
            .unwrap_or((distribution.label.as_str(), ""));
        let Some(budget) = budgets.get(name) else {
            continue;
        };
        let met = distribution.meets(*budget);
        budget_outcomes.insert(distribution.label.clone(), met);
        if !met {
            warnings.push(format!(
                "{case}: {name} p95 {:.1} ms exceeds the declared {budget:.0} ms budget",
                distribution.p95_ms
            ));
        }
    }

    let timings = Timings {
        shape: SHAPE.id(),
        iterations: timing::MEASURED_ITERATIONS,
        warmup: timing::WARMUP_ITERATIONS,
        budgets_ms: budgets,
        distributions,
        budget_outcomes,
    };

    let summary = render(&memory_rows, &timings);
    print!("{summary}");

    if capture {
        let directory = record::write(
            "b3-read",
            PURPOSE,
            corpora.into_values().collect(),
            vec![
                Artifact::identity("memory.json", record::to_json(&memory_rows)),
                Artifact::timings("timings.json", record::to_json(&timings)),
                Artifact::timings("summary.txt", summary),
            ],
            warnings,
        )?;
        eprintln!("captured {}", directory.display());
    }
    let _ = std::fs::remove_dir_all(&revisions_root);
    Ok(())
}

/// Spawn one child per iteration, discarding the warm-up set.
fn cold_samples(
    executable: &Path,
    revisions_root: &Path,
    revision: &str,
    operation: &str,
) -> std::io::Result<Vec<std::time::Duration>> {
    let mut samples = Vec::with_capacity(timing::MEASURED_ITERATIONS);
    let total = timing::WARMUP_ITERATIONS + timing::MEASURED_ITERATIONS;
    for iteration in 0..total {
        let output = std::process::Command::new(executable)
            .arg("--child")
            .arg(revisions_root)
            .arg(revision)
            .arg(operation)
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other(format!(
                "cold child failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        if iteration < timing::WARMUP_ITERATIONS {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let micros: u64 = text
            .trim()
            .parse()
            .map_err(|_| std::io::Error::other(format!("child printed {text:?}")))?;
        samples.push(std::time::Duration::from_micros(micros));
    }
    Ok(samples)
}

/// One cold operation, timed from inside the child so process spawn cost is excluded.
fn child(revisions_root: &str, revision: &str, operation: &str) -> std::io::Result<()> {
    let root = Path::new(revisions_root);
    let published = vec![revision.to_owned()];

    let elapsed = match operation {
        "open" => {
            let started = Instant::now();
            let reader = Reader::open_published(root, revision, &published)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            let elapsed = started.elapsed();
            assert!(reader.manifest().entry_count > 0);
            elapsed
        }
        "search" => {
            let started = Instant::now();
            let mut reader = Reader::open_published(root, revision, &published)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            let index = reader.search_index("english")?;
            let hits = index.query("tech", &[], search::MAX_RESULT_LIMIT);
            let elapsed = started.elapsed();
            assert!(!hits.is_empty());
            elapsed
        }
        "record" => {
            let started = Instant::now();
            let mut reader = Reader::open_published(root, revision, &published)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            let summaries = reader.browse("technology")?;
            let key = summaries.first().expect("a technology exists").key.clone();
            let record = reader.record(&key)?;
            let elapsed = started.elapsed();
            assert!(record.is_some());
            elapsed
        }
        other => return Err(std::io::Error::other(format!("unknown operation {other}"))),
    };

    println!("{}", elapsed.as_micros());
    Ok(())
}

fn render(memory: &[MemoryRow], timings: &Timings) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# b3-read  shape {}\n", timings.shape);

    let _ = writeln!(
        out,
        "{:<34} {:>10} {:>10} {:>10} {:>10} {:>8}",
        "measurement", "median", "p95", "max", "budget", "verdict"
    );
    for distribution in &timings.distributions {
        let name = distribution.label.split(':').next().unwrap_or("");
        let budget = timings.budgets_ms.get(name).copied();
        let _ = writeln!(
            out,
            "{:<34} {:>10.2} {:>10.2} {:>10.2} {:>10} {:>8}",
            distribution.label,
            distribution.median_ms,
            distribution.p95_ms,
            distribution.max_ms,
            budget.map(|b| format!("{b:.0}")).unwrap_or_else(|| "-".into()),
            match budget {
                Some(budget) if distribution.meets(budget) => "ok",
                Some(_) => "MISS",
                None => "-",
            }
        );
    }

    let _ = writeln!(
        out,
        "\n{:<14} {:>8} {:>14} {:>14} {:>14} {:>8}",
        "case", "entries", "index bytes", "browse bytes", "rss delta", "budget"
    );
    for row in memory {
        let _ = writeln!(
            out,
            "{:<14} {:>8} {:>13.2}M {:>13.2}M {:>13.2}M {:>8}",
            row.case,
            row.entries,
            mib(row.index_retained_bytes),
            mib(row.browse_retained_bytes),
            mib(row.rss_delta_bytes),
            if row.meets_budget { "ok" } else { "MISS" },
        );
    }
    out
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
