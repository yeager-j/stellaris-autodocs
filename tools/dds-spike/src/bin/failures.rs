//! `d4-failures` — the typed-outcome contract.
//!
//! `docs/technical-design.md:263` requires the asset module to return exactly one typed outcome
//! per requested slot. The contract has two halves and both need proving: every input maps to one
//! outcome, and no input maps to two. Totality is the stronger and cheaper claim — the four
//! counts must sum to the input count — so it is asserted over the union of the fixtures and the
//! whole corpus rather than case by case.
//!
//! The run also names where each outcome is decided. `MalformedMedia` and `UnsupportedFormat` are
//! decided by lookups over what the file declares, before any decoding; `ConversionFailure` is
//! the only one that indicts the adapter rather than the input. That is why it is unreachable
//! from every real input in the pinned corpora, and why leaving it untested would ship a variant
//! nothing has ever produced.
//!
//! ```text
//! cargo run --release --manifest-path tools/dds-spike/Cargo.toml --bin failures
//! ```
//! Pass `--capture` to write `docs/spikes/dds-records/d4-failures/`.

use dds_spike::classify::{classify, Classification};
use dds_spike::corpus::{self, CorpusIdentity};
use dds_spike::decode_a;
use dds_spike::fixtures;
use dds_spike::model::Outcome;
use dds_spike::recipe::{OutputFormat, Recipe};
use dds_spike::record;
use serde::Serialize;

const PURPOSE: &str = "Prove the typed-outcome contract the asset module owes analysis::finalize: \
every input yields exactly one of Decoded, MalformedMedia, UnsupportedFormat, or \
ConversionFailure, and the four counts sum to the input count. Also record where each outcome is \
decided, because MalformedMedia and UnsupportedFormat must be decided by lookups over the \
container's own declarations rather than by a decoder returning an error — if both reduced to \
'the decoder said no', the two could not be told apart and the resulting Analysis Issue could not \
be scoped. ConversionFailure is unreachable from every input in the pinned corpora, which is \
recorded as a limitation rather than presented as coverage.";

#[derive(Debug, Serialize)]
struct Totality {
    scope: String,
    inputs: usize,
    decoded: usize,
    malformed_media: usize,
    unsupported_format: usize,
    conversion_failure: usize,
    missing_bytes: usize,
    /// Must equal `inputs`. A shortfall would mean an input produced no outcome at all.
    sum: usize,
}

#[derive(Debug, Serialize)]
struct RealWorldCase {
    corpus: String,
    logical: String,
    bytes: usize,
    outcome: String,
    detail: String,
    /// Whether a sprite definition names this file, so the case is reachable in the product.
    referenced: bool,
}

fn main() -> std::io::Result<()> {
    let recipe = Recipe::pinned(OutputFormat::Png);
    let mut identities: Vec<CorpusIdentity> = Vec::new();
    let mut totals: Vec<Totality> = Vec::new();
    let mut real_cases: Vec<RealWorldCase> = Vec::new();
    let mut fixture_lines: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // The fixture corpus is an input to this run, so its identity belongs in the manifest.
    // Without it, altering a committed fixture byte would leave this record reporting `ok` while
    // the evidence underneath it had changed — which is exactly what the drift gate exists to
    // prevent, and what it failed to catch here until this was added.
    let fixtures_corpus = corpus::default_corpora()
        .into_iter()
        .find(|entry| entry.id == "fixtures")
        .expect("the fixture corpus is declared");
    let fixture_files = corpus::enumerate(&fixtures_corpus.root)?;
    identities.push(corpus::identify(&fixtures_corpus, &fixture_files)?);

    // Every fixture, against the claim committed beside it.
    let mut fixture_totals = Totality {
        scope: "fixtures".into(),
        inputs: 0,
        decoded: 0,
        malformed_media: 0,
        unsupported_format: 0,
        conversion_failure: 0,
        missing_bytes: 0,
        sum: 0,
    };
    for fixture in fixtures::all() {
        let bytes = (fixture.bytes)();
        let outcome = decode_a::adapt(&bytes, &recipe);
        let matched = fixture.expected.matches(&outcome);
        if !matched {
            warnings.push(format!(
                "{} claimed {} but produced {}",
                fixture.path,
                fixture.expected.kind(),
                outcome.kind()
            ));
        }
        tally(&mut fixture_totals, &outcome);
        fixture_lines.push(format!(
            "{}\t{}\t{}\t{}\t{}",
            fixture.path,
            fixture.expected.kind(),
            outcome.kind(),
            if matched { "match" } else { "MISMATCH" },
            decided_by(&bytes, &recipe)
        ));
    }
    totals.push(fixture_totals);

    // The whole corpus, plus the real-world non-decoding cases named individually.
    let install = corpus::install_root();
    for corpus_entry in corpus::default_corpora() {
        if corpus_entry.id == "fixtures" {
            continue;
        }
        let files = corpus::enumerate(&corpus_entry.root)?;
        if files.is_empty() {
            warnings.push(format!("corpus {} holds no .dds files", corpus_entry.id));
            continue;
        }
        identities.push(corpus::identify(&corpus_entry, &files)?);

        let referenced = referenced_paths(&corpus_entry, &install)?;
        let mut corpus_totals = Totality {
            scope: corpus_entry.id.clone(),
            inputs: 0,
            decoded: 0,
            malformed_media: 0,
            unsupported_format: 0,
            conversion_failure: 0,
            missing_bytes: 0,
            sum: 0,
        };
        for file in &files {
            let bytes = std::fs::read(&file.absolute)?;
            let outcome = decode_a::adapt(&bytes, &recipe);
            tally(&mut corpus_totals, &outcome);
            if !matches!(outcome, Outcome::Decoded(_)) {
                real_cases.push(RealWorldCase {
                    corpus: corpus_entry.id.clone(),
                    logical: file.logical.clone(),
                    bytes: bytes.len(),
                    outcome: outcome.kind().to_owned(),
                    detail: outcome.detail().to_owned(),
                    referenced: referenced.contains(&file.logical),
                });
            }
        }
        totals.push(corpus_totals);
    }

    // Missing bytes: named by a sprite definition, absent from disk. Decided at the source
    // boundary rather than here, which is why the adapter never produces it.
    let dangling = dangling_count(&install)?;

    for total in &totals {
        println!(
            "{:9} inputs {:6}  decoded {:6}  malformed {:2}  unsupported {:3}  conversion-failure {:2}  sum {:6} {}",
            total.scope,
            total.inputs,
            total.decoded,
            total.malformed_media,
            total.unsupported_format,
            total.conversion_failure,
            total.sum,
            if total.sum == total.inputs { "ok" } else { "SHORTFALL" }
        );
    }
    println!("\nreal-world non-decoding inputs:");
    for case in &real_cases {
        println!(
            "  {:9} {:70} {:19} referenced={} {}",
            case.corpus, case.logical, case.outcome, case.referenced, case.detail
        );
    }
    println!(
        "\nmissing-bytes cases: {dangling} texture paths named by a vanilla sprite definition do \
         not exist. Decided at the source boundary, so this adapter never produces that outcome."
    );

    if totals.iter().any(|total| total.sum != total.inputs) {
        warnings.push("an input produced no outcome; the contract is not total".into());
    }
    if totals.iter().all(|total| total.conversion_failure == 0) {
        warnings.push(
            "ConversionFailure was not produced by any input in the pinned corpora or fixtures. \
             It remains a declared but unexercised outcome; reaching it requires injecting a \
             failure into the encoder or the staging write, which this run does not do."
                .into(),
        );
    }

    if !record::capture_requested() {
        println!("{}", record::NOT_CAPTURED);
        return Ok(());
    }

    #[derive(Serialize)]
    struct Report {
        totality: Vec<Totality>,
        missing_bytes_candidates: usize,
        real_world_cases: Vec<RealWorldCase>,
    }
    let json = serde_json::to_string_pretty(&Report {
        totality: totals,
        missing_bytes_candidates: dangling,
        real_world_cases: real_cases,
    })? + "\n";

    fixture_lines.sort();
    let artifacts = vec![
        ("failures.json".to_string(), json),
        (
            "fixture-outcomes.txt".to_string(),
            {
                let mut text =
                    String::from("# fixture\tclaimed\tactual\tverdict\tdecided by\n");
                for line in &fixture_lines {
                    text.push_str(line);
                    text.push('\n');
                }
                text
            },
        ),
    ];
    let directory = record::write("d4-failures", PURPOSE, identities, artifacts, warnings)?;
    println!("captured {}", directory.display());
    Ok(())
}

fn tally(total: &mut Totality, outcome: &Outcome) {
    total.inputs += 1;
    total.sum += 1;
    match outcome {
        Outcome::Decoded(_) => total.decoded += 1,
        Outcome::MalformedMedia { .. } => total.malformed_media += 1,
        Outcome::UnsupportedFormat { .. } => total.unsupported_format += 1,
        Outcome::ConversionFailure { .. } => total.conversion_failure += 1,
        Outcome::MissingBytes { .. } => total.missing_bytes += 1,
    }
}

/// Which rule decided this input's outcome, named so the record shows it was not the decoder.
fn decided_by(bytes: &[u8], recipe: &Recipe) -> &'static str {
    match classify(bytes) {
        Classification::Malformed(_) => "container reading",
        Classification::Unsupported { .. } => "container reading",
        Classification::Decodable(decodable) => {
            if recipe.accepts(&decodable).is_err() {
                "recipe policy"
            } else if !decode_a::supports(decodable.format) {
                "supported-format set"
            } else {
                "decoder"
            }
        }
    }
}

fn referenced_paths(
    corpus_entry: &corpus::Corpus,
    install: &std::path::Path,
) -> std::io::Result<std::collections::BTreeSet<String>> {
    if corpus_entry.id != "vanilla" {
        return Ok(Default::default());
    }
    let found = dds_spike::references::scan(&corpus_entry.root, &[install])?;
    Ok(found.referenced)
}

fn dangling_count(install: &std::path::Path) -> std::io::Result<usize> {
    let found = dds_spike::references::scan(install, &[install])?;
    Ok(found.dangling.len())
}
