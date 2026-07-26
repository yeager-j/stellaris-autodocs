//! `p2-ranges` — do the recorded byte ranges actually locate the source they claim?
//!
//! Evidence requirement 3 asks for ranges sufficient to show bounded excerpts for top-level
//! definitions and recognized facts, while retaining raw source separately. Three things
//! have to hold, and each is measured rather than argued:
//!
//! 1. **Coverage.** Every node kind carries a range, not just the ones that were easy.
//! 2. **Correctness.** Every range re-slices to exactly the text it claims to cover. A
//!    derivation right about 99% of a corpus is a bug, and only an exhaustive round-trip
//!    separates that from a technique.
//! 3. **Boundedness.** A definition's excerpt has to be small enough to show. The size
//!    distribution says whether that is true of real content or only of fixtures.
//!
//! The tape path is measured alongside, using the unofficial pointer derivation in
//! `tape::scalar_span`, so the finding can state how far the supported tape API alone would
//! have reached rather than asserting that it falls short.
//!
//! ```text
//! cargo run --release --manifest-path tools/parser-spike/Cargo.toml --bin ranges -- --capture
//! ```

use parser_spike::corpus::{self, SourceFile};
use parser_spike::model::{Item, ParsedFile, Value};
use parser_spike::{lexer, record, tape};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

const PURPOSE: &str = "Measure whether the lexer adapter's byte ranges locate the source \
they claim. Every range in every corpus file is re-sliced from the original bytes and \
compared against the token it covers, per node kind, and the size distribution of \
top-level definition spans establishes whether bounded excerpts are practical on real \
content. The tape path is measured beside it through the unofficial borrowed-scalar \
derivation, so the finding can state what the supported tape API reaches on its own.";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Coverage {
    corpus: String,
    files: usize,
    scalars: usize,
    scalars_with_span: usize,
    containers: usize,
    containers_with_span: usize,
    definitions: usize,
    definitions_with_span: usize,
    /// Spans that did not re-slice to the text they claim.
    round_trip_failures: usize,
    /// Definition span sizes in bytes, as an ordered summary.
    definition_bytes_min: usize,
    definition_bytes_median: usize,
    definition_bytes_p95: usize,
    definition_bytes_max: usize,
    /// What the tape reaches through the unofficial pointer derivation.
    tape_files_compared: usize,
    tape_scalars: usize,
    tape_scalars_with_span: usize,
}

#[derive(Default)]
struct FileCoverage {
    scalars: usize,
    scalars_with_span: usize,
    containers: usize,
    containers_with_span: usize,
    definitions: usize,
    definitions_with_span: usize,
    definition_sizes: Vec<usize>,
}

fn walk_items(items: &[Item], into: &mut FileCoverage, top_level: bool) {
    for item in items {
        match item {
            Item::Field(field) => {
                if top_level {
                    into.definitions += 1;
                    if let Some(span) = field.span {
                        into.definitions_with_span += 1;
                        into.definition_sizes.push(span.len());
                    }
                }
                count_scalar(&field.key, into);
                walk_value(&field.value, into);
            }
            Item::Element(value) => walk_value(value, into),
            Item::Conditional(conditional) => walk_items(&conditional.items, into, false),
        }
    }
}

fn walk_value(value: &Value, into: &mut FileCoverage) {
    match value {
        Value::Scalar(scalar) => count_scalar(scalar, into),
        Value::Container(container) => {
            into.containers += 1;
            if container.span.is_some() {
                into.containers_with_span += 1;
            }
            walk_items(&container.items, into, false);
        }
        Value::Tagged { tag, container, .. } => {
            count_scalar(tag, into);
            into.containers += 1;
            if container.span.is_some() {
                into.containers_with_span += 1;
            }
            walk_items(&container.items, into, false);
        }
    }
}

fn count_scalar(scalar: &parser_spike::model::Scalar, into: &mut FileCoverage) {
    into.scalars += 1;
    if scalar.span.is_some() {
        into.scalars_with_span += 1;
    }
}

fn coverage_of(file: &ParsedFile) -> FileCoverage {
    let mut coverage = FileCoverage::default();
    walk_items(&file.items, &mut coverage, true);
    coverage
}

/// What the tape reaches on its own: scalars only, and only through the pointer arithmetic
/// in `tape::scalar_span`, which Jomini does not document. Containers are always `None`
/// there, because no tape token carries the position of a brace.
fn tape_coverage(data: &[u8]) -> Option<(usize, usize)> {
    let parsed = tape::parse(data).ok()?;
    let mut total = 0usize;
    let mut located = 0usize;
    count_tape_items(&parsed.items, &mut total, &mut located);
    Some((total, located))
}

fn count_tape_items(items: &[Item], total: &mut usize, located: &mut usize) {
    for item in items {
        match item {
            Item::Field(field) => {
                *total += 1;
                if field.key.span.is_some() {
                    *located += 1;
                }
                count_tape_value(&field.value, total, located);
            }
            Item::Element(value) => count_tape_value(value, total, located),
            Item::Conditional(conditional) => {
                count_tape_items(&conditional.items, total, located)
            }
        }
    }
}

fn count_tape_value(value: &Value, total: &mut usize, located: &mut usize) {
    match value {
        Value::Scalar(scalar) => {
            *total += 1;
            if scalar.span.is_some() {
                *located += 1;
            }
        }
        Value::Container(container) => count_tape_items(&container.items, total, located),
        Value::Tagged { tag, container, .. } => {
            *total += 1;
            if tag.span.is_some() {
                *located += 1;
            }
            count_tape_items(&container.items, total, located);
        }
    }
}

struct Outcome {
    coverage: FileCoverage,
    round_trip_failures: usize,
    tape: Option<(usize, usize)>,
}

fn examine(file: &SourceFile) -> Option<Outcome> {
    let data = std::fs::read(&file.absolute).ok()?;
    let parsed = lexer::parse(&data);
    Some(Outcome {
        round_trip_failures: lexer::verify_spans(&data, &parsed).len(),
        coverage: coverage_of(&parsed),
        tape: tape_coverage(&data),
    })
}

fn percentile(sorted: &[usize], fraction: f64) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) as f64 * fraction).round() as usize;
    sorted[index]
}

fn main() -> std::io::Result<()> {
    let capture = std::env::args().any(|argument| argument == "--capture");
    let mut reports = Vec::new();
    let mut identities = Vec::new();
    let mut warnings = Vec::new();

    for corpus in corpus::default_corpora() {
        if !corpus.root.is_dir() {
            warnings.push(format!("corpus `{}` not found; skipped", corpus.id));
            continue;
        }
        let files = corpus::enumerate(&corpus.root)?;
        let outcomes: Vec<Outcome> = files.par_iter().filter_map(examine).collect();

        let mut report = Coverage {
            corpus: corpus.id.clone(),
            files: outcomes.len(),
            ..Coverage::default()
        };
        let mut sizes = Vec::new();
        for outcome in &outcomes {
            report.scalars += outcome.coverage.scalars;
            report.scalars_with_span += outcome.coverage.scalars_with_span;
            report.containers += outcome.coverage.containers;
            report.containers_with_span += outcome.coverage.containers_with_span;
            report.definitions += outcome.coverage.definitions;
            report.definitions_with_span += outcome.coverage.definitions_with_span;
            report.round_trip_failures += outcome.round_trip_failures;
            sizes.extend_from_slice(&outcome.coverage.definition_sizes);
            if let Some((total, located)) = outcome.tape {
                report.tape_files_compared += 1;
                report.tape_scalars += total;
                report.tape_scalars_with_span += located;
            }
        }
        sizes.sort_unstable();
        report.definition_bytes_min = sizes.first().copied().unwrap_or(0);
        report.definition_bytes_median = percentile(&sizes, 0.5);
        report.definition_bytes_p95 = percentile(&sizes, 0.95);
        report.definition_bytes_max = sizes.last().copied().unwrap_or(0);

        println!(
            "{:<10} scalars {}/{}  containers {}/{}  definitions {}/{}  roundTripFail {}",
            report.corpus,
            report.scalars_with_span,
            report.scalars,
            report.containers_with_span,
            report.containers,
            report.definitions_with_span,
            report.definitions,
            report.round_trip_failures,
        );
        println!(
            "           definition span bytes: min {} median {} p95 {} max {}",
            report.definition_bytes_min,
            report.definition_bytes_median,
            report.definition_bytes_p95,
            report.definition_bytes_max,
        );
        println!(
            "           tape (unofficial scalar derivation): {}/{} scalars, 0/{} containers",
            report.tape_scalars_with_span, report.tape_scalars, report.containers,
        );

        reports.push(report);
        identities.push(corpus::identify(&corpus, &files)?);
    }

    if !capture {
        eprintln!("(not captured; pass --capture to write the record)");
        return Ok(());
    }

    let mut json = serde_json::to_string_pretty(&reports)?;
    json.push('\n');
    let directory = record::write(
        "p2-ranges",
        PURPOSE,
        identities,
        vec![("ranges.json".to_owned(), json)],
        warnings,
    )?;
    eprintln!("captured {}", directory.display());
    Ok(())
}
