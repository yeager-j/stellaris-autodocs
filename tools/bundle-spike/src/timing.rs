//! The iteration protocol, the declared budgets, and the two readings of retained memory.
//!
//! The evaluation declares its budgets before the deciding measurements are collected, so
//! they live here as constants rather than as numbers a reporting binary chooses after seeing
//! its results. A budget that can be selected after the fact is not a budget.
//!
//! Percentile rank is nearest-rank on the sorted samples, stated rather than implied: with 30
//! iterations, p95 is the 29th of 30 in ascending order. Interpolated percentiles over 30
//! samples invent precision the sample size does not have, and two harnesses disagreeing
//! about which convention they used is exactly the kind of silent difference that makes one
//! spike's numbers incomparable with another's.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Discarded before measurement begins.
///
/// Warm-up exists to stop first-touch page faults, lazy allocator growth, and one-time
/// filesystem metadata reads from being attributed to the operation. It is not a way to reach
/// a nicer number: cold figures are measured in a *separate process* precisely so warm-up
/// cannot quietly launder them.
pub const WARMUP_ITERATIONS: usize = 5;

/// The evaluation requires at least 30 measured iterations after warm-up.
pub const MEASURED_ITERATIONS: usize = 30;

/// The budgets declared in `docs/spikes/revision-bundle-evaluation.md` before capture.
///
/// A failing budget selects sharded JSON, a shared Localization Store, or SQLite according to
/// the measured cause. It is not waived by calling the result responsive, and it may change
/// only before the format outcome is recorded, with a written user-impact rationale and the
/// replacement threshold.
pub mod budget {
    /// Cold revision open through a validated Revision Reader.
    pub const COLD_REVISION_OPEN_MS: f64 = 500.0;
    /// Warm host search at the maximum result limit.
    pub const WARM_SEARCH_MS: f64 = 100.0;
    /// Cold host search after revision open.
    pub const COLD_SEARCH_MS: f64 = 250.0;
    /// Warm documentation-record read.
    pub const WARM_RECORD_READ_MS: f64 = 100.0;
    /// Cold documentation-record read after revision open.
    pub const COLD_RECORD_READ_MS: f64 = 250.0;
    /// Incremental retained memory for active browse and one language's search indexes.
    pub const RETAINED_INDEX_BYTES: u64 = 256 * 1024 * 1024;
    /// Files in one revision bundle.
    pub const BUNDLE_FILES: usize = 10_000;
    /// Complete bundle validation.
    pub const BUNDLE_VALIDATION_MS: f64 = 2_000.0;
    /// Bundle size excluding shared Asset and Localization Stores, as a multiple of the
    /// canonical unsharded read-model payload.
    pub const BUNDLE_SIZE_RATIO: f64 = 2.0;
    /// Bundle size excluding shared Asset and Localization Stores, absolute.
    pub const BUNDLE_SIZE_BYTES: u64 = 1024 * 1024 * 1024;
    /// The threshold that decides the build invocation model: at or under it, an awaited
    /// asynchronous Tauri command is preferred; above it, an explicit host-owned job.
    pub const COMPLETE_BUILD_MS: f64 = 3_000.0;
}

/// A measured latency distribution. Never byte-compared by the drift gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distribution {
    pub label: String,
    pub warmup_iterations: usize,
    pub measured_iterations: usize,
    pub min_ms: f64,
    pub median_ms: f64,
    /// Nearest-rank, not interpolated.
    pub p95_ms: f64,
    pub max_ms: f64,
}

impl Distribution {
    pub fn from_samples(label: impl Into<String>, warmup: usize, samples: &[Duration]) -> Self {
        let mut sorted: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).expect("durations are not NaN"));

        Distribution {
            label: label.into(),
            warmup_iterations: warmup,
            measured_iterations: sorted.len(),
            min_ms: round(sorted.first().copied().unwrap_or(0.0)),
            median_ms: round(nearest_rank(&sorted, 0.50)),
            p95_ms: round(nearest_rank(&sorted, 0.95)),
            max_ms: round(sorted.last().copied().unwrap_or(0.0)),
        }
    }

    /// Whether this distribution's p95 meets a declared budget.
    pub fn meets(&self, budget_ms: f64) -> bool {
        self.p95_ms <= budget_ms
    }
}

/// Nearest-rank percentile over ascending samples.
fn nearest_rank(sorted: &[f64], quantile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (quantile * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

/// Two decimal places. Sub-10-microsecond resolution is below the noise floor of every
/// operation measured here, and printing it suggests a precision the measurement lacks.
fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Run `operation` for warm-up, then for the measured iterations, and summarize.
///
/// `prepare` runs before each iteration and outside the timer. It exists because the first
/// capture of `b1-build` reported warm builds nearly twice as slow as cold ones, which is not
/// a thing that happens: the loop was republishing onto the previous iteration's destination,
/// so every timed iteration silently included deleting the previous bundle's ~700 files.
/// Deletion of a superseded bundle is retention cleanup, which
/// `docs/technical-design.md:549` defers until every handle closes — it is not part of
/// publication and must not be inside a publication measurement.
///
/// The return value of each iteration is passed to `observe` before being dropped, so a
/// caller can assert that the operation actually produced what it claims to have measured.
/// An operation whose result is discarded unlooked-at can be optimized into nothing, and a
/// benchmark of nothing is very fast.
pub fn measure<T>(
    label: impl Into<String>,
    mut prepare: impl FnMut(),
    mut operation: impl FnMut() -> T,
    mut observe: impl FnMut(&T),
) -> Distribution {
    for _ in 0..WARMUP_ITERATIONS {
        prepare();
        let value = operation();
        observe(&value);
    }

    let mut samples = Vec::with_capacity(MEASURED_ITERATIONS);
    for _ in 0..MEASURED_ITERATIONS {
        prepare();
        let start = Instant::now();
        let value = operation();
        samples.push(start.elapsed());
        observe(&value);
    }

    Distribution::from_samples(label, WARMUP_ITERATIONS, &samples)
}

/// Peak resident set size in bytes, as the second and independent reading of retained memory.
///
/// The first reading is the model's own deep byte accounting, which knows exactly what it
/// retained and nothing about allocator overhead or fragmentation. This one knows the
/// opposite. A budget met by one and missed by the other is a finding, not a rounding
/// decision — and the parser spike's `p5-perf` already warned that max-RSS is a
/// whole-process figure, inflated by anything else the process is holding.
pub fn max_rss_bytes() -> u64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return 0;
    }
    // Linux reports kilobytes; macOS and the BSDs report bytes.
    if cfg!(target_os = "linux") {
        (usage.ru_maxrss as u64).saturating_mul(1024)
    } else {
        usage.ru_maxrss as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p95_of_thirty_samples_is_the_twenty_ninth() {
        // 1..=30 ms. Nearest-rank p95 is ceil(0.95 * 30) = 29, so the 29th value.
        let samples: Vec<Duration> = (1..=30).map(|ms| Duration::from_millis(ms)).collect();
        let distribution = Distribution::from_samples("linear", 0, &samples);

        assert_eq!(distribution.p95_ms, 29.0);
        assert_eq!(distribution.max_ms, 30.0);
        assert_eq!(distribution.median_ms, 15.0);
        assert_eq!(distribution.min_ms, 1.0);
    }

    #[test]
    fn a_budget_is_met_at_the_threshold_and_missed_above_it() {
        let at = Distribution::from_samples("at", 0, &[Duration::from_millis(100)]);
        let above = Distribution::from_samples("above", 0, &[Duration::from_micros(100_100)]);

        assert!(at.meets(100.0));
        assert!(!above.meets(100.0));
    }
}
