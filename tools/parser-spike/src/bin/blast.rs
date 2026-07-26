//! `p3-blast` — what does one syntax fault cost?
//!
//! Evidence requirement 4 asks whether a malformed file can be isolated without preventing
//! the rest of the corpus from being indexed, and the spike is explicit that file-level
//! isolation is not the interesting number: it asks how many definitions are lost *per
//! malformed file*, not merely whether other files survive.
//!
//! So this measures loss, on two corpora that answer different questions.
//!
//! The **fixtures** under `fixtures/parser/malformed/` are readable and each states its
//! prediction in its own header, written before this run existed. They are the explanation.
//!
//! **Injected faults** in real files are the scale. A fault is placed at the first, middle,
//! and last definition of each sampled file, because where a fault sits changes what
//! recovery can reach: nothing valid follows a fault at the end, and nothing precedes one at
//! the start. The positive control is the truncation-at-last case, where recovery must be
//! able to do nothing and the lexer must still keep everything before it.
//!
//! ```text
//! cargo run --release --manifest-path tools/parser-spike/Cargo.toml --bin blast -- --capture
//! ```

use parser_spike::corpus::{self, SourceFile};
use parser_spike::model::ParsedFile;
use parser_spike::{corpus::fixtures_root, lexer, record, tape};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

const PURPOSE: &str = "Measure definitions lost per malformed file, rather than whether \
other files still parse. Six hand-written fixtures each carry exactly one fault and state \
their prediction in their own header. Single-character faults are then injected into real \
corpus files at their first, middle, and last definition, and the surviving definition \
count is compared against a clean parse of the same file. The truncation-at-last case is \
the positive control: recovery can reach nothing after it, so the lexer must lose exactly \
the definition that was cut and the tape must lose the file.";

/// One in this many files is sampled from each corpus.
///
/// Injection is quadratic in file size — every fault kind at every position re-parses the
/// whole file — so the whole corpus is not viable. Sampling is by sorted position rather
/// than at random, so the same files are chosen on every run and the record is reproducible.
/// The number of files actually sampled is reported, so a sample can never read as a census.
const SAMPLE_EVERY: usize = 25;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Fault {
    /// Remove the brace that closes the target definition.
    RemoveCloseBrace,
    /// Add a brace immediately after the target definition.
    AddCloseBrace,
    /// Open a quote inside the target definition that never closes.
    OpenQuote,
    /// Cut the file at the start of the target definition.
    Truncate,
}

impl Fault {
    fn name(self) -> &'static str {
        match self {
            Fault::RemoveCloseBrace => "remove_close_brace",
            Fault::AddCloseBrace => "add_close_brace",
            Fault::OpenQuote => "open_quote",
            Fault::Truncate => "truncate",
        }
    }

    const ALL: [Fault; 4] = [
        Fault::RemoveCloseBrace,
        Fault::AddCloseBrace,
        Fault::OpenQuote,
        Fault::Truncate,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Where {
    First,
    Middle,
    Last,
}

impl Where {
    fn name(self) -> &'static str {
        match self {
            Where::First => "first",
            Where::Middle => "middle",
            Where::Last => "last",
        }
    }

    const ALL: [Where; 3] = [Where::First, Where::Middle, Where::Last];

    fn index(self, count: usize) -> usize {
        match self {
            Where::First => 0,
            Where::Middle => count / 2,
            Where::Last => count.saturating_sub(1),
        }
    }
}

/// Apply one fault to one definition, returning the mutated bytes.
fn inject(data: &[u8], parsed: &ParsedFile, fault: Fault, position: Where) -> Option<Vec<u8>> {
    let definitions: Vec<_> = parsed.definitions().filter_map(|field| field.span).collect();
    if definitions.is_empty() {
        return None;
    }
    let span = definitions[position.index(definitions.len())];
    let (start, end) = (span.start as usize, span.end as usize);

    let mut mutated = data.to_vec();
    match fault {
        Fault::RemoveCloseBrace => {
            // The definition's last byte is its closing brace when its value is a container.
            if mutated.get(end - 1) != Some(&b'}') {
                return None;
            }
            mutated.remove(end - 1);
        }
        Fault::AddCloseBrace => mutated.insert(end, b'}'),
        Fault::OpenQuote => {
            // Immediately after the definition's opening brace, so the quote runs into the
            // body rather than merely re-delimiting an identifier. Inserting it inside a
            // name produces `t"ech_x = {…}"`, which is different source but perfectly valid
            // source, and would be counted as a fault that cost nothing.
            let brace = data[start..end].iter().position(|byte| *byte == b'{')?;
            mutated.insert(start + brace + 1, b'"');
        }
        // Only meaningful at the last definition. Cutting at the first or middle deletes
        // every definition after it, which measures content removal rather than the cost of
        // a syntax fault.
        Fault::Truncate if position == Where::Last => mutated.truncate(end.saturating_sub(1)),
        Fault::Truncate => return None,
    }
    Some(mutated)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Cell {
    fault: String,
    position: String,
    cases: usize,
    /// Mutations that neither adapter treated as a fault, and which therefore measure
    /// nothing. Reported rather than dropped: a silently discarded case would make the
    /// remaining averages look like a complete picture.
    not_a_fault: usize,
    baseline_definitions: usize,
    tape_definitions: usize,
    tape_rejected_file: usize,
    lexer_definitions: usize,
    lexer_reported_fault: usize,
}

impl Cell {
    fn tape_retained(&self) -> f64 {
        ratio(self.tape_definitions, self.baseline_definitions)
    }
    fn lexer_retained(&self) -> f64 {
        ratio(self.lexer_definitions, self.baseline_definitions)
    }
    /// The number requirement 4 asks for: definitions lost per malformed file.
    ///
    /// Signed, because recovery can also *gain* top-level definitions. Resuming at a
    /// column-zero line inside a construct the fault had already opened promotes what
    /// follows to top level, so a negative loss is not a rounding artifact — it is
    /// recovery reporting structure that the intact file did not have, and the write-up has
    /// to say so rather than let an unsigned subtraction hide it.
    fn tape_lost_per_file(&self) -> f64 {
        per_case(
            self.baseline_definitions as i64 - self.tape_definitions as i64,
            self.cases,
        )
    }
    fn lexer_lost_per_file(&self) -> f64 {
        per_case(
            self.baseline_definitions as i64 - self.lexer_definitions as i64,
            self.cases,
        )
    }
}

fn ratio(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64 * 100.0
    }
}

fn per_case(total: i64, cases: usize) -> f64 {
    if cases == 0 {
        0.0
    } else {
        total as f64 / cases as f64
    }
}

struct Case {
    fault: Fault,
    position: Where,
    baseline: usize,
    tape: Option<usize>,
    lexer: usize,
    lexer_faulted: bool,
    is_fault: bool,
}

fn examine(file: &SourceFile) -> Vec<Case> {
    let Ok(data) = std::fs::read(&file.absolute) else {
        return Vec::new();
    };
    let clean = lexer::parse(&data);
    // A file that is already broken cannot measure the cost of breaking it.
    if !clean.faults.is_empty() || tape::parse(&data).is_err() {
        return Vec::new();
    }
    let baseline = clean.definitions().count();
    if baseline < 3 {
        return Vec::new();
    }

    let mut cases = Vec::new();
    for fault in Fault::ALL {
        for position in Where::ALL {
            let Some(mutated) = inject(&data, &clean, fault, position) else {
                continue;
            };
            let lexed = lexer::parse(&mutated);
            let taped = tape::parse(&mutated).ok();
            // A mutation that neither adapter treats as a fault did not create one. It is
            // recorded and set aside rather than averaged in as a fault that cost nothing.
            let is_fault = !lexed.faults.is_empty() || taped.is_none();
            cases.push(Case {
                fault,
                position,
                baseline,
                tape: taped.map(|file| file.definitions().count()),
                lexer: lexed.definitions().count(),
                lexer_faulted: !lexed.faults.is_empty(),
                is_fault,
            });
        }
    }
    cases
}

fn fixtures_report() -> std::io::Result<String> {
    let directory = fixtures_root().join("malformed");
    let mut names: Vec<_> = std::fs::read_dir(&directory)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "txt"))
        .collect();
    names.sort();

    let mut out = String::from("# fixture\ttape\tlexer\tfaults\tabandoned\n");
    for path in names {
        let data = std::fs::read(&path)?;
        let tape = match tape::parse(&data) {
            Ok(file) => format!("{}", file.definitions().count()),
            Err(_) => "rejected".to_string(),
        };
        let lexed = lexer::parse(&data);
        let abandoned: u32 = lexed.faults.iter().map(|fault| fault.abandoned).sum();
        out.push_str(&format!(
            "{}\t{tape}\t{}\t{}\t{abandoned}\n",
            path.file_name().unwrap_or_default().to_string_lossy(),
            lexed.definitions().count(),
            lexed.faults.len(),
        ));
        for fault in &lexed.faults {
            out.push_str(&format!(
                "\t\u{21b3} {} at byte {} resumed {:?}\n",
                fault.message, fault.offset, fault.resumed_at
            ));
        }
    }
    Ok(out)
}

fn main() -> std::io::Result<()> {
    let capture = std::env::args().any(|argument| argument == "--capture");

    let fixtures = fixtures_report()?;
    println!("malformed fixtures\n{fixtures}");

    let mut cells: Vec<Cell> = Vec::new();
    let mut identities = Vec::new();
    let mut warnings = Vec::new();
    let mut sampled_total = 0usize;
    let mut available_total = 0usize;

    for corpus in corpus::default_corpora() {
        if !corpus.root.is_dir() {
            warnings.push(format!("corpus `{}` not found; skipped", corpus.id));
            continue;
        }
        let files = corpus::enumerate(&corpus.root)?;
        available_total += files.len();
        let sample: Vec<_> = files
            .iter()
            .step_by(SAMPLE_EVERY)
            .cloned()
            .collect();
        sampled_total += sample.len();
        eprintln!(
            "{}: sampling {} of {} files (every {}th by sorted logical path)",
            corpus.id,
            sample.len(),
            files.len(),
            SAMPLE_EVERY
        );

        let cases: Vec<Case> = sample.par_iter().flat_map(examine).collect();
        for case in cases {
            let cell = cells
                .iter_mut()
                .find(|cell| cell.fault == case.fault.name() && cell.position == case.position.name());
            let cell = match cell {
                Some(cell) => cell,
                None => {
                    cells.push(Cell {
                        fault: case.fault.name().to_owned(),
                        position: case.position.name().to_owned(),
                        ..Cell::default()
                    });
                    cells.last_mut().expect("just pushed")
                }
            };
            if !case.is_fault {
                cell.not_a_fault += 1;
                continue;
            }
            cell.cases += 1;
            cell.baseline_definitions += case.baseline;
            match case.tape {
                Some(count) => cell.tape_definitions += count,
                None => cell.tape_rejected_file += 1,
            }
            cell.lexer_definitions += case.lexer;
            if case.lexer_faulted {
                cell.lexer_reported_fault += 1;
            }
        }
    }

    cells.sort_by(|a, b| (a.fault.clone(), a.position.clone()).cmp(&(b.fault.clone(), b.position.clone())));

    println!(
        "injected faults over {sampled_total} sampled files of {available_total} available\n"
    );
    println!(
        "{:<20} {:<8} {:>6} {:>7} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "fault", "at", "cases", "noFault", "tapeKept", "tapeLost", "tapeRejct", "lexKept", "lexLost"
    );
    for cell in &cells {
        println!(
            "{:<20} {:<8} {:>6} {:>7} {:>8.1}% {:>9.1} {:>9} {:>8.1}% {:>9.1}",
            cell.fault,
            cell.position,
            cell.cases,
            cell.not_a_fault,
            cell.tape_retained(),
            cell.tape_lost_per_file(),
            cell.tape_rejected_file,
            cell.lexer_retained(),
            cell.lexer_lost_per_file(),
        );
    }
    println!(
        "\n`tapeLost` and `lexLost` are mean definitions lost per malformed file, which is \
what evidence requirement 4 asks for. `tapeRejct` counts files the tape refused outright, \
where every definition is lost. `noFault` counts mutations neither adapter treated as a \
fault, excluded from the averages. A negative loss means recovery resumed inside a \
construct the fault had opened and promoted its contents to top level."
    );

    if !capture {
        eprintln!("\n(not captured; pass --capture to write the record)");
        return Ok(());
    }

    for corpus in corpus::default_corpora() {
        if corpus.root.is_dir() {
            let files = corpus::enumerate(&corpus.root)?;
            identities.push(corpus::identify(&corpus, &files)?);
        }
    }

    let mut json = serde_json::to_string_pretty(&cells)?;
    json.push('\n');
    let directory = record::write(
        "p3-blast",
        PURPOSE,
        identities,
        vec![
            ("injected.json".to_owned(), json),
            ("fixtures.txt".to_owned(), fixtures),
            (
                "sampling.txt".to_owned(),
                format!(
                    "every {SAMPLE_EVERY}th file by sorted logical path\n\
                     sampled {sampled_total} of {available_total} files\n\
                     files already faulty, already rejected by the tape, or holding fewer \
                     than three definitions are excluded from injection because they cannot \
                     measure the cost of a new fault\n"
                ),
            ),
        ],
        warnings,
    )?;
    eprintln!("captured {}", directory.display());
    Ok(())
}
