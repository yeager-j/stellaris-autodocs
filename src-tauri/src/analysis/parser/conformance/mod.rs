//! Whole-corpus parser conformance: reading every script file of a real Stellaris
//! installation and checking that the answer is exact, stable, and independently
//! corroborated.
//!
//! Fixtures explain a defect once you already suspect it. The corpus is what discriminates:
//! five of the parser spike's eight wrapper defects appeared only at corpus scale, and one
//! class — a silent structural misread — has no detector inside a single reading at all
//! (`docs/spikes/parser-evaluation.md`). This run therefore asks three questions of every
//! file:
//!
//! 1. **Does every derived byte range still cut the token it claims?** Re-sliced through
//!    [`super::ranges`], which has its own negative control.
//! 2. **Is the parsed model's identity stable?** The corpus digest must not depend on the
//!    order files are folded in, on whether the work ran on one thread or many, or on where
//!    the tree happens to live on disk.
//! 3. **Does a second, independent reading agree?** [`tape`] reads the same bytes through
//!    `jomini::TextTape`, sharing none of the adapter's trivia or dialect code, and the two
//!    structural projections are compared over the files both readings accept.
//!
//! # Running it
//!
//! The corpora are a licensed local installation, so this is not part of the ordinary suite:
//! plain `cargo test` never selects it, and normal CI does not need Stellaris installed.
//!
//! ```text
//! cargo test --features test-support corpus_conformance -- --ignored --nocapture
//! ```
//!
//! Roots are environment-overridable — `STELLARIS_INSTALL_ROOT` and
//! `STELLARIS_WORKSHOP_ROOT`, defaulting to the macOS Steam locations. If the run is asked
//! for and a root is missing it **fails**; it does not skip. A gate that quietly reports
//! success when it verified nothing is worse than no gate.
//!
//! Pass `PARSER_CONFORMANCE_CAPTURE=1` to write the record under `docs/conformance/parser/`.
//! Without it the run checks against the committed record and writes nothing.
//!
//! # Re-run it when
//!
//! - the **Stellaris build** changes,
//! - **Jomini** is upgraded,
//! - the **dialect lexer** in `super::jomini` is edited.
//!
//! The same standard `docs/adr/0008` holds for a texture-decoder upgrade. Each of the three
//! changes what the corpus means or what reading it produces, and the drift gate is how the
//! difference gets stated rather than discovered later in generated documentation.
//!
//! # Negative controls
//!
//! Every gate here has been shown to fail:
//!
//! - the range check, by [`super::ranges::the_range_check_detects_a_shifted_span`];
//! - the cross-check, by [`a_seeded_structural_misread_fails_the_cross_check`], which runs
//!   in ordinary CI over the committed fixture corpus;
//! - the drift gate, twice — once against a corpus and once against the record itself.
//!
//! ```text
//! STELLARIS_WORKSHOP_ROOT=/tmp/acot-with-one-added-byte \
//!   cargo test --features test-support corpus_conformance -- --ignored --nocapture
//!
//! corpus acot: fingerprint 6f6095c2… -> 5e12cd18… (1120 files, 18521905 bytes recorded;
//!                                                   425 files, 8745791 bytes observed)
//! ```
//!
//! and, after appending one line to a captured artifact:
//!
//! ```text
//! artifact divergences.txt: edited after capture
//! ```

mod expected;
mod record;
mod tape;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::ranges::{RangeFault, verify_ranges};
use super::{ParsedFile, SourceIdentity, digest};
use crate::canonical::encode::DigestBytes;
use crate::discovery::proposals;
use crate::source::fingerprint::ContentHash;
use crate::source::policy::FileFamily;
use crate::source::snapshot::{
    Established, LiveSource, SourceBytes, SourceKind, SourceSnapshot, establish,
};
use record::CorpusIdentity;
use tape::{Divergence, Pairing};

const RUN: &str = "c1-parser-conformance";

const PURPOSE: &str = "\
Parse every enumerated script file of the installed Vanilla and ACOT corpora through the \
production adapter, re-slice every derived byte range against the source it claims to cover, \
and compare the parsed model against an independent reading of the same bytes through \
Jomini's TextTape. The two readings share no trivia or dialect code, so a structural \
disagreement over a file both accept is a defect in one of them rather than a difference of \
opinion — and it is the only detector for a misread that leaves the source syntactically \
valid. The corpus digest is recomputed under a reversed fold and under serial execution, \
because an identity that depended on scheduling would not be an identity.";

/// ACOT's Workshop identifier. The Workshop addresses mods by number, and the number is what
/// a record can be reproduced from; the title is for humans.
const ACOT_WORKSHOP_ID: &str = "1419304439";

struct Corpus {
    id: &'static str,
    title: &'static str,
    kind: SourceKind,
    root: PathBuf,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate directory has a parent")
        .to_path_buf()
}

/// An environment override, or a `~`-expanded default. An empty value counts as unset, so
/// exporting the variable empty does not silently point the run at `/`.
fn env_path(name: &str, default: &str) -> PathBuf {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => expand_home(default),
    }
}

fn expand_home(raw: &str) -> PathBuf {
    match raw.strip_prefix("~/") {
        Some(rest) => match std::env::var("HOME") {
            Ok(home) => PathBuf::from(home).join(rest),
            Err(_) => PathBuf::from(raw),
        },
        None => PathBuf::from(raw),
    }
}

fn install_root() -> PathBuf {
    env_path(
        "STELLARIS_INSTALL_ROOT",
        "~/Library/Application Support/Steam/steamapps/common/Stellaris",
    )
}

fn workshop_root() -> PathBuf {
    env_path(
        "STELLARIS_WORKSHOP_ROOT",
        "~/Library/Application Support/Steam/steamapps/workshop/content/281990",
    )
}

/// The two local corpora, in the order records report them.
///
/// Vanilla is the content every mod is documented against; ACOT is the Target Mod the
/// product's golden cases are built around, and the corpus that produced the stray-token
/// finding the cross-check exists for.
fn local_corpora() -> Vec<Corpus> {
    vec![
        Corpus {
            id: "vanilla",
            title: "Stellaris base game",
            kind: SourceKind::VanillaContent,
            root: install_root(),
        },
        Corpus {
            id: "acot",
            title: "Ancient Cache of Technologies",
            kind: SourceKind::TargetMod,
            root: workshop_root().join(ACOT_WORKSHOP_ID),
        },
    ]
}

/// The committed fixture tree, shaped like a mod so it establishes through exactly the
/// production enumeration a real corpus does.
fn fixture_corpus() -> Corpus {
    Corpus {
        id: "fixtures",
        title: "Hand-authored parser fixtures (valid)",
        kind: SourceKind::TargetMod,
        root: repo_root().join("fixtures").join("parser").join("valid"),
    }
}

/// What the second reading said about one file.
#[derive(Debug)]
enum Comparison {
    Agreed,
    Diverged(Divergence),
    /// The tape refused the file outright. An ordinary outcome: it rejects real dialect the
    /// adapter handles, and those files simply cannot be compared.
    TapeRejected(String),
    /// The adapter recovered from a fault. Excluded from the structural comparison — a
    /// repaired file is expected to differ from a reshaped one — but counted, because
    /// without that count a wrapper defect showing up as a spurious fault would be invisible
    /// in the divergence total.
    AdapterRecovered,
}

struct FileReport {
    identity: SourceIdentity,
    digest: DigestBytes,
    range_faults: Vec<RangeFault>,
    comparison: Comparison,
}

fn examine(identity: SourceIdentity, data: &[u8], pairing: Pairing) -> FileReport {
    let file: ParsedFile = super::parse(identity.clone(), data);
    let range_faults = verify_ranges(data, &file);
    let digest = super::parsed_file_digest(&file);
    let comparison = match tape::read(data, pairing) {
        Err(rejection) => Comparison::TapeRejected(rejection.message),
        Ok(_) if !file.faults.is_empty() => Comparison::AdapterRecovered,
        Ok(shapes) => match tape::first_divergence(&shapes, &tape::project(&file)) {
            Some(divergence) => Comparison::Diverged(divergence),
            None => Comparison::Agreed,
        },
    };
    FileReport {
        identity,
        digest,
        range_faults,
        comparison,
    }
}

/// One corpus's whole outcome: its identity, its digest under three computations, and every
/// individual finding worth listing.
struct Conformance {
    id: &'static str,
    identity: CorpusIdentity,
    compared: usize,
    agreed: usize,
    /// The corpus digest as the run computed it: parallel examination, forward fold.
    digest: DigestBytes,
    /// The same per-file digests folded in the opposite order. Order-independence is a
    /// property of the fold, so re-folding is what tests it; re-parsing would not.
    digest_reversed_fold: DigestBytes,
    /// A complete second pass on one thread. Tests per-file parse determinism under
    /// scheduling, which re-folding cannot.
    digest_serial: DigestBytes,
    range_faults: Vec<String>,
    divergences: Vec<(String, Divergence)>,
    tape_rejections: Vec<String>,
    recoveries: Vec<String>,
    warnings: Vec<String>,
}

fn run(corpus: &Corpus, pairing: Pairing) -> Conformance {
    let (source, warnings) = establish_corpus(corpus);
    let snapshot = source.snapshot();
    let files = script_files(snapshot);

    let reports = examine_parallel(&files, pairing);
    let entries: Vec<(SourceIdentity, DigestBytes)> = reports
        .iter()
        .map(|report| (report.identity.clone(), report.digest))
        .collect();

    let mut conformance = Conformance {
        id: corpus.id,
        identity: identify(corpus, snapshot, &files),
        compared: 0,
        agreed: 0,
        digest: digest::corpus_of(entries.iter().cloned()),
        digest_reversed_fold: digest::corpus_of(entries.iter().rev().cloned()),
        digest_serial: digest_serial(&files),
        range_faults: Vec::new(),
        divergences: Vec::new(),
        tape_rejections: Vec::new(),
        recoveries: Vec::new(),
        warnings,
    };

    for report in reports {
        let logical = report.identity.logical.as_str().to_owned();
        for fault in report.range_faults {
            conformance.range_faults.push(format!(
                "{logical}\t{}..{} claims {}",
                fault.range.start, fault.range.end, fault.claim
            ));
        }
        match report.comparison {
            Comparison::Agreed => {
                conformance.compared += 1;
                conformance.agreed += 1;
            }
            Comparison::Diverged(divergence) => {
                conformance.compared += 1;
                conformance.divergences.push((logical, divergence));
            }
            Comparison::TapeRejected(message) => {
                conformance
                    .tape_rejections
                    .push(format!("{logical}\t{message}"));
            }
            Comparison::AdapterRecovered => conformance.recoveries.push(logical),
        }
    }
    conformance
}

fn establish_corpus(corpus: &Corpus) -> (LiveSource, Vec<String>) {
    assert!(
        corpus.root.is_dir(),
        "corpus {} is not installed at {}. Set STELLARIS_INSTALL_ROOT / \
         STELLARIS_WORKSHOP_ROOT, or do not ask for this run: a missing corpus is a failed \
         run, never a silent pass.",
        corpus.id,
        corpus.root.display()
    );
    let established = establish(corpus.kind, &corpus.root)
        .unwrap_or_else(|error| panic!("corpus {} could not be established: {error}", corpus.id));
    match established {
        Established::Complete(source) => (source, Vec::new()),
        // Publishable, and worth recording: a gap is content the observation could not see,
        // so a run over an incomplete corpus is measuring slightly less than it claims.
        Established::Incomplete(source) => {
            let warning = format!(
                "corpus {} established incomplete: {:?}",
                corpus.id,
                source.snapshot().gaps()
            );
            (source, vec![warning])
        }
    }
}

/// The script files of a snapshot, in canonical order, paired with the exact bytes that were
/// hashed. Localization is a different language with a different owner and is not parsed
/// here (`source::policy`).
fn script_files(snapshot: &SourceSnapshot) -> Vec<(SourceIdentity, SourceBytes)> {
    snapshot
        .paths()
        .filter(|logical| snapshot.family(logical) == Some(FileFamily::Script))
        .map(|logical| {
            let bytes = snapshot
                .read(logical)
                .expect("an enumerated path reads back from its own snapshot");
            (SourceIdentity::new(snapshot.kind(), logical.clone()), bytes)
        })
        .collect()
}

fn examine_parallel(files: &[(SourceIdentity, SourceBytes)], pairing: Pairing) -> Vec<FileReport> {
    let threads = std::thread::available_parallelism().map_or(1, |count| count.get());
    let chunk = files.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = files
            .chunks(chunk)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|(identity, bytes)| examine(identity.clone(), bytes, pairing))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("no examination panicked"))
            .collect()
    })
}

fn digest_serial(files: &[(SourceIdentity, SourceBytes)]) -> DigestBytes {
    digest::corpus_of(files.iter().map(|(identity, bytes)| {
        let parsed = super::parse(identity.clone(), bytes);
        (identity.clone(), super::parsed_file_digest(&parsed))
    }))
}

fn identify(
    corpus: &Corpus,
    snapshot: &SourceSnapshot,
    files: &[(SourceIdentity, SourceBytes)],
) -> CorpusIdentity {
    CorpusIdentity {
        id: corpus.id.to_owned(),
        title: corpus.title.to_owned(),
        file_count: files.len(),
        total_bytes: files.iter().map(|(_, bytes)| bytes.len() as u64).sum(),
        fingerprint: snapshot.fingerprint().to_hex(),
        files: record::is_committed(&corpus.root).then(|| {
            files
                .iter()
                .map(|(identity, bytes)| {
                    (
                        identity.logical.as_str().to_owned(),
                        ContentHash::of(bytes).to_hex(),
                    )
                })
                .collect()
        }),
    }
}

/// The digest of the fixture tree copied to a different absolute path.
///
/// A copy, never a symlink: a symlink shares the inode, so the check would pass even if the
/// code had canonicalized the path and folded the absolute location into the identity.
fn digest_at_a_different_root(corpus: &Corpus) -> DigestBytes {
    let temporary = tempfile::TempDir::new().expect("a temporary directory");
    let copied = temporary.path().join(corpus.id);
    copy_tree(&corpus.root, &copied);
    let moved = Corpus {
        id: corpus.id,
        title: corpus.title,
        kind: corpus.kind,
        root: copied,
    };
    let (source, _) = establish_corpus(&moved);
    digest_serial(&script_files(source.snapshot()))
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("a destination directory");
    for entry in fs::read_dir(from).expect("a readable source directory") {
        let entry = entry.expect("a readable directory entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("a copyable file");
        }
    }
}

/// Everything a run says, as the lines a record and a failure message both want.
fn listing(title: &str, corpus: &str, lines: &[String]) -> String {
    // The total is printed before the lines, so a list that is ever truncated cannot read as
    // complete.
    format!(
        "# {title} — {corpus}: {} total\n{}",
        lines.len(),
        lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>()
    )
}

fn failures(runs: &[Conformance]) -> Vec<String> {
    let mut failures = Vec::new();
    for conformance in runs {
        let corpus = conformance.id;
        if !conformance.range_faults.is_empty() {
            failures.push(listing("range faults", corpus, &conformance.range_faults));
        }
        if conformance.digest != conformance.digest_reversed_fold {
            failures.push(format!(
                "{corpus}: the corpus digest depends on fold order ({} vs {})",
                conformance.digest, conformance.digest_reversed_fold
            ));
        }
        if conformance.digest != conformance.digest_serial {
            failures.push(format!(
                "{corpus}: the corpus digest depends on scheduling ({} vs {})",
                conformance.digest, conformance.digest_serial
            ));
        }
        let reconciliation = expected::reconcile(corpus, &conformance.divergences);
        if !reconciliation.is_clean() {
            failures.push(listing(
                "unexpected divergences",
                corpus,
                &reconciliation.unexpected,
            ));
            failures.push(listing(
                "pinned divergences that no longer occur",
                corpus,
                &reconciliation.absent,
            ));
        }
    }
    failures
}

fn summary(conformance: &Conformance) -> String {
    format!(
        "{}: {} files, digest {}, {} compared ({} agreed, {} diverged), \
         {} tape rejections, {} adapter recoveries",
        conformance.id,
        conformance.identity.file_count,
        conformance.digest,
        conformance.compared,
        conformance.agreed,
        conformance.divergences.len(),
        conformance.tape_rejections.len(),
        conformance.recoveries.len(),
    )
}

#[test]
fn the_fixture_corpus_conforms_and_diverges_only_where_pinned() {
    let conformance = run(&fixture_corpus(), Pairing::Faithful);
    let failures = failures(std::slice::from_ref(&conformance));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        conformance.agreed > 0,
        "the cross-check agreed about nothing, so it proved nothing"
    );
}

#[test]
fn a_seeded_structural_misread_fails_the_cross_check() {
    // The negative control for the cross-check, and the one that runs without a Stellaris
    // installation. `Perturbed` mispairs one key and value in the second reading: both
    // readings stay individually plausible and differ only in structure, which is exactly
    // the defect class no single reading can detect.
    //
    // Stated as "every file that agreed now disagrees" rather than "some divergence exists",
    // because a pinned divergence already exists in this corpus and a weaker assertion would
    // pass on that alone.
    let corpus = fixture_corpus();
    let faithful = run(&corpus, Pairing::Faithful);
    let seeded = run(&corpus, Pairing::Perturbed);

    assert!(faithful.agreed > 0, "there was no agreement to break");
    assert_eq!(
        seeded.compared, faithful.compared,
        "the seeded misread changed which files were comparable, so it is not the \
         structure-only fault this control claims to be"
    );
    assert_eq!(
        seeded.agreed, 0,
        "the cross-check did not notice a seeded structural misread in {} of {} files",
        seeded.agreed, seeded.compared
    );
}

#[test]
fn a_corpus_digest_survives_fold_order_scheduling_and_a_different_absolute_root() {
    let corpus = fixture_corpus();
    let conformance = run(&corpus, Pairing::Faithful);
    let expected = conformance.digest.to_hex();

    assert_eq!(
        conformance.digest_reversed_fold.to_hex(),
        expected,
        "the parsed-corpus identity depends on the order files are folded in"
    );
    assert_eq!(
        conformance.digest_serial.to_hex(),
        expected,
        "the parsed-corpus identity depends on scheduling"
    );
    assert_eq!(
        digest_at_a_different_root(&corpus).to_hex(),
        expected,
        "the parsed-corpus identity depends on where the tree lives"
    );
}

/// The drift-checked local-corpus run. See the module comment for how to invoke it and when
/// to re-run it.
#[test]
#[ignore = "requires an installed Stellaris and ACOT; run with --ignored"]
fn corpus_conformance() {
    let corpora = local_corpora();
    let runs: Vec<Conformance> = corpora
        .iter()
        .map(|corpus| run(corpus, Pairing::Faithful))
        .collect();

    for conformance in &runs {
        println!("{}", summary(conformance));
        for warning in &conformance.warnings {
            println!("warning: {warning}");
        }
    }

    let failures = failures(&runs);
    assert!(failures.is_empty(), "{}", failures.join("\n"));

    let stellaris = installed_build();
    let environment = record::environment(stellaris);
    let identities: Vec<CorpusIdentity> = runs
        .iter()
        .map(|conformance| conformance.identity.clone())
        .collect();

    // Capture is the deliberate act of accepting a new state, so it reports drift rather
    // than failing on it — otherwise the one instruction a drift failure gives would be
    // unreachable, and a record could never be brought back into step with an updated game.
    // The conformance checks above are unconditional either way: a failing run is never
    // recorded.
    let capturing = record::capture_requested();
    let recorded = record::read(RUN);
    match &recorded {
        Some(recorded) => {
            let drift = record::drift(recorded, &environment, &identities);
            let location = record::records_root().join(RUN);
            assert!(
                drift.is_empty() || capturing,
                "the run drifted from {}:\n{}\n\nDrift is not always wrong: a game update, a \
                 Jomini upgrade, or a toolchain bump all belong here. Re-capture with \
                 PARSER_CONFORMANCE_CAPTURE=1 once the change is understood.",
                location.display(),
                drift.join("\n")
            );
            for reason in drift {
                println!("drift accepted by capture: {reason}");
            }
        }
        None => assert!(
            capturing,
            "no record exists at {}. Capture one with PARSER_CONFORMANCE_CAPTURE=1 — an \
             absent record must not read as an undrifted run.",
            record::records_root().join(RUN).display()
        ),
    }

    if capturing {
        let written = capture(&runs, environment, identities);
        println!("captured {}", written.display());
    }
}

fn installed_build() -> BTreeMap<String, String> {
    let install = proposals::read_installed_build(install_root());
    [
        ("version", install.version),
        ("rawVersion", install.raw_version),
        (
            "modsCompatibilityVersion",
            install.mods_compatibility_version,
        ),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value)))
    .collect()
}

fn capture(
    runs: &[Conformance],
    environment: record::Environment,
    identities: Vec<CorpusIdentity>,
) -> PathBuf {
    let mut conformance = serde_json::Map::new();
    let mut divergences = String::new();
    let mut rejections = String::new();
    let mut recoveries = String::new();
    let mut warnings = Vec::new();

    for run in runs {
        conformance.insert(
            run.id.to_owned(),
            serde_json::json!({
                "files": run.identity.file_count,
                "total_bytes": run.identity.total_bytes,
                "parsed_corpus_digest": run.digest.to_hex(),
                "compared": run.compared,
                "agreed": run.agreed,
                "diverged": run.divergences.len(),
                "tape_rejected": run.tape_rejections.len(),
                "adapter_recovered": run.recoveries.len(),
                "range_faults": run.range_faults.len(),
            }),
        );
        let listed: Vec<String> = run
            .divergences
            .iter()
            .map(|(logical, divergence)| {
                format!("{logical}\t{}: {}", divergence.path, divergence.detail)
            })
            .collect();
        divergences.push_str(&listing("divergences", run.id, &listed));
        rejections.push_str(&listing("tape rejections", run.id, &run.tape_rejections));
        recoveries.push_str(&listing("adapter recoveries", run.id, &run.recoveries));
        warnings.extend(run.warnings.iter().cloned());
    }

    let mut json = serde_json::to_string_pretty(&serde_json::Value::Object(conformance))
        .expect("a report of plain data");
    json.push('\n');

    record::write(
        RUN,
        PURPOSE,
        environment,
        identities,
        &[
            ("conformance.json", json),
            ("divergences.txt", divergences),
            ("tape-rejections.txt", rejections),
            ("recoveries.txt", recoveries),
        ],
        warnings,
    )
    .expect("the record directory is writable")
}
