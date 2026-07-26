//! `p1-coverage` — parse every corpus file both ways and compare.
//!
//! Answers evidence requirements 1, 2, 5, and 6: whether every syntactically valid file
//! parses, whether order, duplicates, operators, and mixed containers survive, whether
//! unfamiliar constructs pass through as data, and whether the model stayed
//! parser-independent.
//!
//! The comparison is the point. Path A is Jomini's mature front end and Path B is a wrapper
//! written for this spike, so every file both accept must produce the same model modulo
//! spans. The number and shape of the divergences is the evidence for whether that wrapper
//! is maintainable or is quietly a second parser.
//!
//! ```text
//! cargo run --release --manifest-path tools/parser-spike/Cargo.toml --bin coverage
//! ```
//!
//! Pass `--capture` to write `docs/spikes/parser-records/p1-coverage/`.

use parser_spike::corpus::{self, Corpus, SourceFile};
use parser_spike::digest::{digest, first_divergence};
use parser_spike::survey::{Census, Encoding};
use parser_spike::{lexer, record, tape};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const PURPOSE: &str = "Parse every file of every pinned corpus through both adapters and \
compare the results. Path A is Jomini's TextTape; Path B is this spike's TokenReader \
wrapper, which adds source spans and fault recovery. Every file both adapters accept must \
produce an identical application-owned model once spans are excluded, so a divergence is a \
defect in the wrapper rather than a difference of opinion. The run also censuses which \
syntax the corpus actually contains, because a coverage claim over source that never used a \
comparison operator or a mixed container would have no denominator.";

/// How many divergent and failing files to name individually.
///
/// Bounded so a record stays readable, and the totals are always reported separately, so a
/// truncated list can never read as a complete one.
const LISTED: usize = 40;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Report {
    corpus: String,
    files: usize,
    bytes: u64,
    tape_parsed: usize,
    tape_failed: usize,
    lexer_clean: usize,
    lexer_with_faults: usize,
    /// Files both adapters accepted whose models differ.
    divergent: usize,
    /// Files the tape accepted but the lexer had to recover from.
    ///
    /// Counted separately because these never reach the structural comparison — a recovered
    /// file is expected to differ — so without this line a wrapper defect that manifests as
    /// a spurious fault would be invisible in the divergence count.
    unexpected_faults: usize,
    /// Files whose every span re-sliced to the text it claims to cover.
    spans_verified: usize,
    span_faults: usize,
    encodings: BTreeMap<String, usize>,
    tape_tokens: BTreeMap<String, usize>,
    census: Census,
    files_with_duplicate_top_level_definitions: usize,
}

struct FileOutcome {
    logical: String,
    bytes: u64,
    encoding: Encoding,
    tape_error: Option<String>,
    divergence: Option<String>,
    span_faults: Vec<String>,
    lexer_faults: Vec<String>,
    census: Census,
    tokens: Option<parser_spike::tape::TokenCensus>,
    duplicates: usize,
}

fn examine(file: &SourceFile) -> Option<FileOutcome> {
    let data = std::fs::read(&file.absolute).ok()?;
    let encoding = Encoding::of(&data);

    let lexed = lexer::parse(&data);
    let span_faults = lexer::verify_spans(&data, &lexed)
        .iter()
        .map(|fault| {
            format!(
                "{}..{} expected {:?} found {:?}",
                fault.span.start, fault.span.end, fault.expected, fault.found
            )
        })
        .collect();
    let lexer_faults = lexed
        .faults
        .iter()
        .map(|fault| {
            format!(
                "{} at byte {} resumed {:?} abandoned {}",
                fault.message, fault.offset, fault.resumed_at, fault.abandoned
            )
        })
        .collect();
    let census = Census::of(&lexed);

    let (tape_error, divergence) = match tape::parse(&data) {
        Err(error) => (Some(error.message), None),
        Ok(taped) => {
            // Only compare where the tape succeeded and the lexer had nothing to recover
            // from: a file the lexer repaired is expected to differ from one the tape
            // reshaped, and counting that as a wrapper defect would bury the real ones.
            let divergence = if lexed.faults.is_empty() {
                first_divergence(&taped, &lexed).map(|found| {
                    format!("{}: {}", found.path, found.detail)
                })
            } else {
                None
            };
            if divergence.is_none() && lexed.faults.is_empty() {
                debug_assert_eq!(digest(&taped), digest(&lexed));
            }
            (None, divergence)
        }
    };

    Some(FileOutcome {
        logical: file.logical.clone(),
        bytes: data.len() as u64,
        encoding,
        tape_error,
        lexer_faults,
        divergence,
        span_faults,
        duplicates: census.duplicate_top_level_definitions,
        census,
        tokens: tape::token_census(&data).ok(),
    })
}

fn survey(corpus: &Corpus, files: &[SourceFile]) -> (Report, Lists) {
    let outcomes: Vec<FileOutcome> = files.par_iter().filter_map(examine).collect();

    let mut report = Report {
        corpus: corpus.id.clone(),
        files: outcomes.len(),
        ..Report::default()
    };
    let mut divergences = Vec::new();
    let mut failures = Vec::new();
    let mut spans = Vec::new();
    let mut recovered = Vec::new();

    for outcome in &outcomes {
        report.bytes += outcome.bytes;
        *report
            .encodings
            .entry(outcome.encoding.name().to_owned())
            .or_default() += 1;

        match &outcome.tape_error {
            Some(message) => {
                report.tape_failed += 1;
                failures.push(format!("{}\t{message}", outcome.logical));
            }
            None => report.tape_parsed += 1,
        }

        if outcome.lexer_faults.is_empty() {
            report.lexer_clean += 1;
        } else {
            report.lexer_with_faults += 1;
            if outcome.tape_error.is_none() {
                report.unexpected_faults += 1;
            }
            let expected = if outcome.tape_error.is_none() {
                "tape-accepted"
            } else {
                "tape-rejected"
            };
            for fault in &outcome.lexer_faults {
                recovered.push(format!("{}\t{expected}\t{fault}", outcome.logical));
            }
        }

        if let Some(detail) = &outcome.divergence {
            report.divergent += 1;
            divergences.push(format!("{}\t{detail}", outcome.logical));
        }

        if outcome.span_faults.is_empty() {
            report.spans_verified += 1;
        } else {
            report.span_faults += outcome.span_faults.len();
            for fault in &outcome.span_faults {
                spans.push(format!("{}\t{fault}", outcome.logical));
            }
        }

        if outcome.duplicates > 0 {
            report.files_with_duplicate_top_level_definitions += 1;
        }

        report.census.merge(&outcome.census);
        if let Some(tokens) = &outcome.tokens {
            for (name, count) in [
                ("array", tokens.arrays),
                ("object", tokens.objects),
                ("mixed_container", tokens.mixed_containers),
                ("header", tokens.headers),
                ("parameter", tokens.parameters),
                ("undefined_parameter", tokens.undefined_parameters),
                ("operator", tokens.operators),
                ("quoted", tokens.quoted),
                ("unquoted", tokens.unquoted),
            ] {
                *report.tape_tokens.entry(name.to_owned()).or_default() += count;
            }
        }
    }

    divergences.sort();
    failures.sort();
    spans.sort();
    recovered.sort();
    (report, Lists { divergences, failures, spans, recovered })
}

/// Per-corpus detail lists, emitted as record artifacts.
struct Lists {
    divergences: Vec<String>,
    failures: Vec<String>,
    spans: Vec<String>,
    recovered: Vec<String>,
}

fn main() -> std::io::Result<()> {
    let capture = std::env::args().any(|argument| argument == "--capture");

    let mut reports = Vec::new();
    let mut identities = Vec::new();
    let mut divergence_lines = Vec::new();
    let mut failure_lines = Vec::new();
    let mut span_lines = Vec::new();
    let mut recovery_lines = Vec::new();
    let mut warnings = Vec::new();

    for corpus in corpus::default_corpora() {
        if !corpus.root.is_dir() {
            warnings.push(format!(
                "corpus `{}` not found at {}; it contributed nothing to this run",
                corpus.id,
                corpus.root.display()
            ));
            eprintln!("warning: skipping `{}`: no {}", corpus.id, corpus.root.display());
            continue;
        }

        let files = corpus::enumerate(&corpus.root)?;
        eprintln!("{}: {} files", corpus.id, files.len());

        let (report, lists) = survey(&corpus, &files);
        divergence_lines.push((corpus.id.clone(), lists.divergences));
        failure_lines.push((corpus.id.clone(), lists.failures));
        span_lines.push((corpus.id.clone(), lists.spans));
        recovery_lines.push((corpus.id.clone(), lists.recovered));
        reports.push(report);
        identities.push(corpus::identify(&corpus, &files)?);
    }

    print_summary(&reports);

    if !capture {
        eprintln!("\n(not captured; pass --capture to write the record)");
        return Ok(());
    }

    let artifacts = vec![
        ("coverage.json".to_owned(), to_json(&reports)?),
        ("divergences.txt".to_owned(), listing(&divergence_lines)),
        ("tape-failures.txt".to_owned(), listing(&failure_lines)),
        ("span-faults.txt".to_owned(), listing(&span_lines)),
        ("lexer-recoveries.txt".to_owned(), listing(&recovery_lines)),
    ];
    let directory = record::write("p1-coverage", PURPOSE, identities, artifacts, warnings)?;
    eprintln!("captured {}", directory.display());
    Ok(())
}

fn to_json<T: Serialize>(value: &T) -> std::io::Result<String> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    Ok(text)
}

/// Render per-corpus lists, always stating the true total before any truncation.
fn listing(groups: &[(String, Vec<String>)]) -> String {
    let mut out = String::new();
    for (corpus, lines) in groups {
        out.push_str(&format!("# {corpus}: {} total\n", lines.len()));
        for line in lines.iter().take(LISTED) {
            out.push_str(line);
            out.push('\n');
        }
        if lines.len() > LISTED {
            out.push_str(&format!(
                "# … {} more not listed\n",
                lines.len() - LISTED
            ));
        }
        out.push('\n');
    }
    out
}

fn print_summary(reports: &[Report]) {
    println!(
        "\n{:<10} {:>7} {:>9} {:>7} {:>7} {:>7} {:>9} {:>7}",
        "corpus", "files", "MiB", "tapeOK", "tapeERR", "diverge", "spanFault", "unexpFlt"
    );
    for report in reports {
        println!(
            "{:<10} {:>7} {:>9.1} {:>7} {:>7} {:>7} {:>9} {:>7}",
            report.corpus,
            report.files,
            report.bytes as f64 / 1_048_576.0,
            report.tape_parsed,
            report.tape_failed,
            report.divergent,
            report.span_faults,
            report.unexpected_faults,
        );
    }

    println!("\nsyntax census (model)");
    for report in reports {
        println!(
            "  {:<10} definitions={} maxDepth={} tagged={} conditionals={} dupTopLevel={} lexerFaultFiles={}",
            report.corpus,
            report.census.definitions,
            report.census.max_depth,
            report.census.tagged_values,
            report.census.conditionals,
            report.census.duplicate_top_level_definitions,
            report.lexer_with_faults,
        );
        println!("    operators   {:?}", report.census.operators);
        println!("    scalars     {:?}", report.census.scalar_kinds);
        println!("    containers  {:?}", report.census.container_kinds);
        println!("    encodings   {:?}", report.encodings);
        println!("    tapeTokens  {:?}", report.tape_tokens);
    }
}
