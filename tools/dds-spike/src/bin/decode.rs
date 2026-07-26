//! `d2-decode` — decode every file in the pinned corpora twice and compare.
//!
//! The correctness run. Path A is `image_dds`. Path B reinterprets uncompressed layouts straight
//! from the file's own masks and decodes BC1, BC2, and BC3 from the S3TC specification. On every
//! input both paths accept, their mip-0 RGBA8 must agree, and every disagreement is either a
//! defect or a finding.
//!
//! The agreement count is not the evidence. Two readings that share an assumption agree while both
//! being wrong, so the run's gates are deliberate faults injected into path B. Each one computes,
//! per file, whether the fault *could* change that file's pixels, and then asserts that the set of
//! files which actually diverged is exactly the set which should have. Both halves matter: a file
//! that should have diverged and did not is a blind spot, and a file that diverged when the fault
//! could not have reached it means the harness is measuring something other than the fault.
//!
//! - `--inject swap-rb` reads the blue mask where red belongs. Every uncompressed file whose red
//!   and blue channels are not already equal in every pixel must diverge; greyscale icons, masks,
//!   and flat backgrounds cannot be changed by the fault and must not be counted against it.
//! - `--inject assume-bgra` ignores the declared masks and reads every 32-bit surface as the
//!   majority layout — what a decoder keyed on bit count rather than on masks does, which is the
//!   most likely way to be wrong here and the way that yields a plausible image. Only the files
//!   declaring the opposite channel order should diverge. If none did, the corpus would contain
//!   nothing able to tell a mask-reading decoder from a table-reading one, and every agreement
//!   number in this spike would be measuring a check that could not have failed.
//!
//! ```text
//! cargo run --release --manifest-path tools/dds-spike/Cargo.toml --bin decode
//! cargo run --release --manifest-path tools/dds-spike/Cargo.toml --bin decode -- --inject swap-rb
//! cargo run --release --manifest-path tools/dds-spike/Cargo.toml --bin decode -- --inject assume-bgra
//! ```
//! Pass `--capture` to write `docs/spikes/dds-records/d2-decode/`.

use std::collections::BTreeMap;

use dds_spike::classify::{classify, Classification, SourceFormat};
use dds_spike::corpus::{self, CorpusIdentity};
use dds_spike::decode_a;
use dds_spike::decode_b;
use dds_spike::model::{compare, Comparison};
use dds_spike::recipe::{OutputFormat, Recipe};
use dds_spike::record;
use rayon::prelude::*;
use serde::Serialize;

const PURPOSE: &str = "Decode every file in the pinned corpora through two independent readings \
and compare their mip-0 RGBA8 byte for byte. Path A is image_dds. Path B reinterprets \
uncompressed layouts from the DDS pixel-format masks and decodes BC1, BC2, and BC3 from the S3TC \
specification, which is a stronger independent reading than a second library because it is the \
statement both libraries are implementing. The run also records one typed outcome per input, so \
the outcome contract is measured over the whole corpus rather than over the fixtures alone. Its \
gates are two injected faults, each asserting that the set of files which diverged is exactly the \
set the fault could have changed.";

/// The largest per-channel difference treated as a rounding disagreement rather than a defect.
///
/// Two conformant BC decoders need not produce identical bytes: `bcdec_rs` folds the 5-bit-to-
/// 8-bit expansion into its interpolation with one rounding, and this harness expands first and
/// interpolates second with another. Both are legitimate. The threshold is stated here rather
/// than discovered, and the run reports the observed maximum beside it so a reader can see how
/// much headroom the claim actually has.
const ROUNDING_TOLERANCE: u8 = 4;

#[derive(Debug, Default, Serialize)]
struct Coverage {
    corpus: String,
    files: usize,
    outcomes: BTreeMap<String, usize>,
    identical: usize,
    within_rounding: usize,
    beyond_rounding: usize,
    dimensions_differ: usize,
    outcomes_differ: usize,
    both_failed: usize,
    not_compared: usize,
    /// The largest per-channel delta seen anywhere in this corpus, and where.
    max_delta: [u8; 4],
    max_delta_path: String,
}

/// The result of a deliberate fault injected into path B.
#[derive(Debug, Serialize)]
struct Injection {
    mode: String,
    /// Files the fault could act on at all.
    applicable: usize,
    /// Of those, the ones whose pixels the fault could actually change.
    should_diverge: usize,
    /// Should have diverged and did. Equal to `should_diverge` when the control passes.
    diverged_as_expected: usize,
    /// Should have diverged and did not: inputs the cross-check is blind on.
    missed: Vec<String>,
    /// Diverged although the fault could not reach them: the harness measuring something else.
    spurious: Vec<String>,
}

impl Injection {
    fn passes(&self) -> bool {
        self.missed.is_empty() && self.spurious.is_empty()
    }
}

fn main() -> std::io::Result<()> {
    let inject = std::env::args()
        .position(|argument| argument == "--inject")
        .and_then(|index| std::env::args().nth(index + 1));
    let recipe = Recipe::pinned(OutputFormat::Png);

    let mut identities: Vec<CorpusIdentity> = Vec::new();
    let mut coverages: Vec<Coverage> = Vec::new();
    let mut divergence_lines: Vec<String> = Vec::new();
    let mut outcome_lines: Vec<String> = Vec::new();
    let mut decoded_count = 0usize;
    let mut warnings: Vec<String> = Vec::new();
    let mut injection = Injection {
        mode: inject.clone().unwrap_or_default(),
        applicable: 0,
        should_diverge: 0,
        diverged_as_expected: 0,
        missed: Vec::new(),
        spurious: Vec::new(),
    };

    for corpus_entry in corpus::default_corpora() {
        let files = corpus::enumerate(&corpus_entry.root)?;
        identities.push(corpus::identify(&corpus_entry, &files)?);
        if files.is_empty() {
            warnings.push(format!("corpus {} holds no .dds files", corpus_entry.id));
            continue;
        }

        let results: Vec<FileResult> = files
            .par_iter()
            .map(|file| {
                let bytes = std::fs::read(&file.absolute).unwrap_or_default();
                evaluate(&file.logical, &bytes, &recipe, inject.as_deref())
            })
            .collect();

        let mut coverage = Coverage {
            corpus: corpus_entry.id.clone(),
            files: results.len(),
            ..Default::default()
        };

        for result in &results {
            *coverage
                .outcomes
                .entry(result.outcome_kind.clone())
                .or_default() += 1;
            // Only the inputs that did not decode are listed. Naming all 33,104 that did would
            // add three megabytes repeating a word the histogram in `coverage.json` already
            // counts, and would bury the 41 rows that carry the information.
            if result.outcome_kind != "decoded" {
                outcome_lines.push(format!(
                    "{}\t{}\t{}\t{}\t{}",
                    corpus_entry.id,
                    result.logical,
                    result.format,
                    result.outcome_kind,
                    result.detail
                ));
            } else {
                decoded_count += 1;
            }
            tally(&mut coverage, result, &corpus_entry.id, &mut divergence_lines);

            if let Some(injected) = &result.injected {
                injection.applicable += 1;
                let path = format!("{}\t{}", corpus_entry.id, result.logical);
                match (injected.should_diverge, injected.diverged) {
                    (true, true) => {
                        injection.should_diverge += 1;
                        injection.diverged_as_expected += 1;
                    }
                    (true, false) => {
                        injection.should_diverge += 1;
                        injection.missed.push(path);
                    }
                    (false, true) => injection.spurious.push(path),
                    (false, false) => {}
                }
            }
        }

        println!(
            "{:9} files {:6}  identical {:6}  rounding {:5}  beyond {:3}  dims {:2}  outcome {:3}  failed {:2}",
            coverage.corpus,
            coverage.files,
            coverage.identical,
            coverage.within_rounding,
            coverage.beyond_rounding,
            coverage.dimensions_differ,
            coverage.outcomes_differ,
            coverage.both_failed
        );
        for (kind, count) in &coverage.outcomes {
            println!("           {kind:20} {count:6}");
        }
        if coverage.max_delta.iter().any(|delta| *delta > 0) {
            println!(
                "           worst per-channel delta {:?} at {}",
                coverage.max_delta, coverage.max_delta_path
            );
        }
        coverages.push(coverage);
    }

    if let Some(mode) = &inject {
        injection.missed.sort();
        injection.spurious.sort();
        println!(
            "\ninjection {mode}: {} applicable, {} could be changed by the fault, {} caught",
            injection.applicable, injection.should_diverge, injection.diverged_as_expected
        );
        for path in injection.missed.iter().take(20) {
            println!("   MISSED (fault not detected): {path}");
        }
        for path in injection.spurious.iter().take(20) {
            println!("   SPURIOUS (fault could not reach it): {path}");
        }
        println!(
            "control {}",
            if injection.passes() {
                "PASSES: the diverging set is exactly the set the fault could change"
            } else {
                "FAILS"
            }
        );
        // An injected run measures a deliberate fault, not the corpus. Capturing it as `d2-decode`
        // would overwrite the real result with one produced by broken code.
        if record::capture_requested() {
            println!("refusing to capture an injected run");
        }
        return Ok(());
    }

    if !record::capture_requested() {
        println!("{}", record::NOT_CAPTURED);
        return Ok(());
    }

    #[derive(Serialize)]
    struct Report {
        rounding_tolerance: u8,
        corpora: Vec<Coverage>,
    }
    let coverage_json = serde_json::to_string_pretty(&Report {
        rounding_tolerance: ROUNDING_TOLERANCE,
        corpora: coverages,
    })? + "\n";

    divergence_lines.sort();
    outcome_lines.sort();
    let artifacts = vec![
        ("coverage.json".to_string(), coverage_json),
        (
            "divergences.txt".to_string(),
            table("# corpus\tlogical path\tformat\tdetail", &divergence_lines),
        ),
        (
            "outcomes.txt".to_string(),
            table(
                &format!(
                    "# every input that did not decode. {decoded_count} further inputs decoded \
                     and are counted in coverage.json rather than listed here.\n# corpus\tlogical \
                     path\tformat\toutcome\tdetail"
                ),
                &outcome_lines,
            ),
        ),
    ];
    let directory = record::write("d2-decode", PURPOSE, identities, artifacts, warnings)?;
    println!("captured {}", directory.display());
    Ok(())
}

fn tally(
    coverage: &mut Coverage,
    result: &FileResult,
    corpus: &str,
    divergences: &mut Vec<String>,
) {
    match &result.comparison {
        Comparison::Identical => coverage.identical += 1,
        Comparison::PixelsDiffer {
            differing_pixels,
            total_pixels,
            max_delta,
        } => {
            let worst = max_delta.iter().copied().max().unwrap_or(0);
            if worst <= ROUNDING_TOLERANCE {
                coverage.within_rounding += 1;
            } else {
                coverage.beyond_rounding += 1;
                divergences.push(format!(
                    "{corpus}\t{}\t{}\t{differing_pixels} of {total_pixels} pixels\tmax delta rgba {max_delta:?}",
                    result.logical, result.format
                ));
            }
            if worst > coverage.max_delta.iter().copied().max().unwrap_or(0) {
                coverage.max_delta = *max_delta;
                coverage.max_delta_path = result.logical.clone();
            }
        }
        Comparison::DimensionsDiffer { a, b } => {
            coverage.dimensions_differ += 1;
            divergences.push(format!(
                "{corpus}\t{}\t{}\tdimensions {a:?} vs {b:?}",
                result.logical, result.format
            ));
        }
        Comparison::OutcomesDiffer { a, b } => {
            coverage.outcomes_differ += 1;
            divergences.push(format!(
                "{corpus}\t{}\t{}\toutcomes {a} vs {b}",
                result.logical, result.format
            ));
        }
        Comparison::BothFailed { .. } => coverage.both_failed += 1,
        Comparison::NotCompared { .. } => coverage.not_compared += 1,
    }
}

struct FileResult {
    logical: String,
    format: String,
    outcome_kind: String,
    detail: String,
    comparison: Comparison,
    injected: Option<InjectionResult>,
}

struct InjectionResult {
    /// The fault could change this file's decoded pixels.
    should_diverge: bool,
    diverged: bool,
}

fn evaluate(logical: &str, bytes: &[u8], recipe: &Recipe, inject: Option<&str>) -> FileResult {
    let classification = classify(bytes);
    let format = classification.label();
    let a = decode_a::adapt(bytes, recipe);
    let b = decode_b::adapt(bytes, recipe);
    let comparison = compare(&a, &b);

    let injected = inject.and_then(|mode| match &classification {
        Classification::Decodable(decodable) if recipe.accepts(decodable).is_ok() => {
            let SourceFormat::Uncompressed(layout) = decodable.format else {
                return None;
            };
            // Red and blue equal in every pixel: no channel-order fault can change this image,
            // so it must not be counted as a miss.
            let invariant = b
                .decoded()
                .is_some_and(|image| image.rgba8.chunks_exact(4).all(|px| px[0] == px[2]));

            let (faulty, reachable) = match mode {
                "swap-rb" => (
                    decode_b::decode_with_swapped_red_blue(bytes, decodable),
                    true,
                ),
                "assume-bgra" => {
                    if layout.bit_count != 32 {
                        // The fault only rewrites 32-bit surfaces; a 24-bit or 16-bit file is
                        // outside its reach and is not evidence either way.
                        return None;
                    }
                    let majority = (
                        (layout.red.shift, layout.red.bits),
                        (layout.blue.shift, layout.blue.bits),
                    ) == ((16, 8), (0, 8));
                    (
                        decode_b::decode_assuming_majority_layout(bytes, decodable),
                        !majority,
                    )
                }
                _ => return None,
            };

            Some(InjectionResult {
                should_diverge: reachable && !invariant,
                diverged: compare(&a, &faulty).is_divergence(),
            })
        }
        _ => None,
    });

    FileResult {
        logical: logical.to_owned(),
        format,
        outcome_kind: a.kind().to_owned(),
        detail: a.detail().to_owned(),
        comparison,
        injected,
    }
}

fn table(header: &str, rows: &[String]) -> String {
    let mut text = String::from(header);
    text.push('\n');
    for row in rows {
        text.push_str(row);
        text.push('\n');
    }
    text
}
