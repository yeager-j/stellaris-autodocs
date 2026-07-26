//! `p5-perf` — how long does parsing the corpus take, and how much memory does it hold?
//!
//! Evidence requirement 7. These numbers are directional: they come from a spike harness,
//! not from the real adapter inside a build, and `docs/technical-design.md:428` already
//! applies that standard to the fingerprint measurements this sits beside. What they are
//! good for is ruling a design in or out — a parser that took a minute over vanilla would
//! change the build model regardless of how it was measured.
//!
//! Each configuration runs several times and reports the spread rather than a single
//! number, because one reading cannot distinguish a fast parser from a warm page cache.
//! The first run of each corpus is discarded for exactly that reason.
//!
//! ```text
//! cargo run --release --manifest-path tools/parser-spike/Cargo.toml --bin perf -- --capture
//! ```

use parser_spike::corpus::{self, SourceFile};
use parser_spike::{lexer, record, tape};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const PURPOSE: &str = "Time and memory for parsing each pinned corpus through both \
adapters, serially and in parallel. Each configuration is repeated and reported as a range \
rather than a single reading, and the first run of each corpus is discarded so a cold page \
cache is not attributed to the parser. Directional, in the same sense the technical design \
already applies to its fingerprint measurements: sufficient to rule a design in or out, not \
a substitute for measuring the real adapter in a real build.";

const REPEATS: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Timing {
    corpus: String,
    path: String,
    mode: String,
    files: usize,
    bytes: u64,
    repeats: usize,
    min_ms: f64,
    median_ms: f64,
    max_ms: f64,
    mib_per_second: f64,
}

/// Peak resident set size for this process, in bytes.
///
/// `ru_maxrss` is a high-water mark for the whole process, so it can only ever grow. It is
/// reported once at the end rather than per configuration, because attributing it to one
/// measurement would imply a per-configuration figure the call cannot provide.
fn peak_resident_bytes() -> u64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return 0;
    }
    let maximum = usage.ru_maxrss as u64;
    // macOS reports bytes; Linux reports kibibytes.
    if cfg!(target_os = "macos") {
        maximum
    } else {
        maximum * 1024
    }
}

fn load(files: &[SourceFile]) -> Vec<Vec<u8>> {
    files
        .iter()
        .filter_map(|file| std::fs::read(&file.absolute).ok())
        .collect()
}

fn time(repeats: usize, mut body: impl FnMut()) -> Vec<Duration> {
    let mut samples = Vec::with_capacity(repeats);
    for _ in 0..repeats {
        let started = Instant::now();
        body();
        samples.push(started.elapsed());
    }
    samples.sort();
    samples
}

fn summarize(
    corpus: &str,
    path: &str,
    mode: &str,
    files: usize,
    bytes: u64,
    samples: &[Duration],
) -> Timing {
    let ms = |duration: &Duration| duration.as_secs_f64() * 1000.0;
    let median = ms(&samples[samples.len() / 2]);
    Timing {
        corpus: corpus.to_owned(),
        path: path.to_owned(),
        mode: mode.to_owned(),
        files,
        bytes,
        repeats: samples.len(),
        min_ms: ms(&samples[0]),
        median_ms: median,
        max_ms: ms(&samples[samples.len() - 1]),
        mib_per_second: if median > 0.0 {
            (bytes as f64 / 1_048_576.0) / (median / 1000.0)
        } else {
            0.0
        },
    }
}

fn main() -> std::io::Result<()> {
    let capture = std::env::args().any(|argument| argument == "--capture");
    let mut timings = Vec::new();
    let mut identities = Vec::new();
    let mut warnings = Vec::new();

    println!(
        "{:<10} {:<6} {:<9} {:>7} {:>9} {:>9} {:>9} {:>9}",
        "corpus", "path", "mode", "files", "min ms", "median", "max ms", "MiB/s"
    );

    for corpus in corpus::default_corpora() {
        if !corpus.root.is_dir() {
            warnings.push(format!("corpus `{}` not found; skipped", corpus.id));
            continue;
        }
        let files = corpus::enumerate(&corpus.root)?;
        // Read once, up front. Timing the parser against filesystem reads would measure the
        // disk, and the build's own snapshot protocol reads the bytes separately anyway
        // (docs/technical-design.md:410).
        let contents = load(&files);
        let bytes: u64 = contents.iter().map(|data| data.len() as u64).sum();

        // Discard: warms the allocator and the page cache so the reported spread is the
        // parser's rather than the first touch of 50 MiB.
        contents.iter().for_each(|data| {
            let _ = lexer::parse(data);
        });

        for (path, parallel) in [("lexer", false), ("lexer", true), ("tape", false), ("tape", true)]
        {
            let samples = time(REPEATS, || {
                if parallel {
                    contents.par_iter().for_each(|data| match path {
                        "lexer" => {
                            let _ = lexer::parse(data);
                        }
                        _ => {
                            let _ = tape::parse(data);
                        }
                    });
                } else {
                    contents.iter().for_each(|data| match path {
                        "lexer" => {
                            let _ = lexer::parse(data);
                        }
                        _ => {
                            let _ = tape::parse(data);
                        }
                    });
                }
            });

            let mode = if parallel { "parallel" } else { "serial" };
            let timing = summarize(&corpus.id, path, mode, files.len(), bytes, &samples);
            println!(
                "{:<10} {:<6} {:<9} {:>7} {:>9.1} {:>9.1} {:>9.1} {:>9.1}",
                timing.corpus,
                timing.path,
                timing.mode,
                timing.files,
                timing.min_ms,
                timing.median_ms,
                timing.max_ms,
                timing.mib_per_second,
            );
            timings.push(timing);
        }

        identities.push(corpus::identify(&corpus, &files)?);
    }

    let peak = peak_resident_bytes();
    println!(
        "\npeak resident set for the whole run: {:.1} MiB ({} repeats per configuration)",
        peak as f64 / 1_048_576.0,
        REPEATS
    );
    println!(
        "process-wide high-water mark, not attributable to any one configuration; every \
corpus is held in memory at once here, which a real build would not do"
    );

    if !capture {
        eprintln!("(not captured; pass --capture to write the record)");
        return Ok(());
    }

    let mut json = serde_json::to_string_pretty(&timings)?;
    json.push('\n');
    let directory = record::write(
        "p5-perf",
        PURPOSE,
        identities,
        vec![
            ("timings.json".to_owned(), json),
            (
                "memory.txt".to_owned(),
                format!(
                    "peak_resident_bytes\t{peak}\n\
                     note\tprocess-wide high-water mark across every corpus and both \
                     adapters, measured with getrusage(RUSAGE_SELF). Not a per-corpus \
                     figure, and inflated by holding all corpora in memory at once.\n"
                ),
            ),
        ],
        warnings,
    )?;
    eprintln!("captured {}", directory.display());
    Ok(())
}
