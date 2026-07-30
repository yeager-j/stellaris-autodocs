//! The captured run record and the drift comparison that makes it a gate, shared by every
//! corpus-facing conformance harness.
//!
//! Two harnesses capture records over the installed corpora — the parser's whole-corpus
//! conformance run ([`super::parser::conformance`], `c1`) and the resolver's parse-and-resolve
//! run ([`super::resolver::conformance`], `c2`) — and both have to agree on what a record is:
//! its manifest shape, its environment block, how a corpus is identified, and what drifting
//! from a record means. Homed here, beside [`super::corpora`], for the same reason the corpus
//! locations are: neither harness may become a second authority on the format the other is
//! compared under. Keeping the corpus identity and environment blocks identical across runs is
//! what lets one drift vocabulary serve both.
//!
//! A record is what lets a later run say "the same corpus, read by the same code, still
//! produces the same answer" — and, when it does not, name which of those three moved. It
//! holds identities and counts only: **no corpus content is ever copied into the
//! repository.** Logical paths and digests are what a licensed local installation needs to
//! reproduce a run, and they are all a record carries for a corpus outside the repo.
//!
//! The manifest is written last, hashing the artifacts already on disk, so a manifest can
//! never name a file that was not produced. The shape follows the spike records under
//! `docs/spikes/*-records/` deliberately; Phase 4M extended it with the optional
//! [`ResolutionRecord`] block rather than inventing a second format.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::corpora::{Corpus, install_root, repo_root};
use crate::discovery::proposals;
use crate::source::fingerprint::ContentHash;
use crate::source::policy::FileFamily;
use crate::source::snapshot::SourceSnapshot;

/// The Jomini requirement these harnesses link against.
///
/// Duplicated from `Cargo.toml` rather than read from it, so a record states the version the
/// code was compiled with; `the_recorded_jomini_version_matches_the_manifest` keeps the two
/// honest.
const JOMINI_VERSION: &str = "0.35";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::analysis) struct Manifest {
    pub run: String,
    pub purpose: String,
    pub environment: Environment,
    pub corpora: Vec<CorpusRecord>,
    /// The resolver's outcome over the corpus pair. Present only for a parse-and-resolve
    /// run; the parser conformance run records corpora outcomes alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<ResolutionRecord>,
    pub artifacts: BTreeMap<String, String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::analysis) struct Environment {
    /// From `launcher-settings.json`, never from `game.log`: the log banner records the last
    /// run rather than the installed build, and the two disagree whenever logs are stale.
    pub stellaris: BTreeMap<String, String>,
    pub jomini: String,
    pub rustc: String,
    pub os: String,
    pub arch: String,
}

/// What a corpus *is*: the same identity a build derives from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::analysis) struct CorpusIdentity {
    pub id: String,
    pub title: String,
    pub file_count: usize,
    pub total_bytes: u64,
    /// The production `/v3` source fingerprint — the same identity a build derives, so a
    /// corpus that drifts here is a corpus that would change a revision.
    pub fingerprint: String,
    /// Per-file digests, for corpora inside the repository only. A licensed local
    /// installation is identified by its fingerprint and counts; enumerating its paths would
    /// put a shipped product's file listing in this repository for no verification gain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<BTreeMap<String, String>>,
}

/// What the parser conformance run *got* from a corpus.
///
/// Recorded and drift-compared, not merely reported, and that is the point of it. The
/// corpus fingerprint answers "did the source move" and the environment answers "did the
/// tools move"; **neither answers "did the parser start reading the same bytes
/// differently"** — which is precisely what the recurrence trigger's third case, a
/// dialect-lexer edit, causes. A change in `ScalarKind`, in a derived range that still
/// re-slices correctly, or in evidence quality is invisible to the structural cross-check by
/// design, and would otherwise reach generated documentation with every gate green.
///
/// `parsed_corpus_digest` is the value that closes that: it covers ranges, scalar kinds,
/// operators, evidence quality, and faults over every file of the corpus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::analysis) struct Outcome {
    pub parsed_corpus_digest: String,
    pub compared: usize,
    pub agreed: usize,
    pub diverged: usize,
    pub tape_rejected: usize,
    pub adapter_recovered: usize,
    pub range_faults: usize,
}

/// One corpus as a record holds it: what it is, and — for a run that cross-checks every
/// parsed file — what reading it produced. The parse-and-resolve run identifies its corpora
/// the same way and records its outcome in [`ResolutionRecord`] instead, because resolution
/// is an answer about the *pair*, not about either corpus alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::analysis) struct CorpusRecord {
    #[serde(flatten)]
    pub identity: CorpusIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
}

/// What a parse-and-resolve run said about every declared Resolution Profile row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::analysis) struct ResolutionRecord {
    /// The profile the rows were resolved under. A version bump with an unchanged outcome is
    /// still drift: the record's numbers were produced under a policy that no longer exists.
    pub profile_version: u32,
    pub rows: Vec<RowRecord>,
    pub localization: LocalizationOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::analysis) struct RowRecord {
    pub registry: String,
    pub outcome: RowOutcome,
}

/// One row's whole outcome: it resolved, or it refused with a typed reason.
///
/// There is no third variant, deliberately — the same "no silent fallback" contract the
/// resolver states. A run that panicked instead of recording a refusal would make the open
/// cells this record exists to witness unrecordable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(in crate::analysis) enum RowOutcome {
    Resolved {
        definitions: usize,
        /// Whole files this row's scope lost to common file selection.
        removed_files: usize,
        /// A deterministic digest of the row's complete resolved output — every definition's
        /// position, body, effective fields, provenance, and typed facts. The counts below
        /// are diagnostics; this is the identity, for the same reason the parser run records
        /// `parsed_corpus_digest`: a winner swap or a moved provenance site can leave every
        /// count unchanged while the documentation input differs.
        semantic_digest: String,
        /// Every typed count the resolution produced — provenance kinds, detected references,
        /// constant and inline and sprite outcomes — resolved and unresolved alike, keyed
        /// `domain.Variant`.
        facts: BTreeMap<String, usize>,
        /// The subset of `facts` that are typed visible failures: unresolved constants,
        /// omitted inclusions, sprites without an effective texture — each recorded here
        /// rather than failing the run.
        visible_failures: BTreeMap<String, usize>,
    },
    Refused {
        /// Which policy cell (or refusal kind) the row stopped at.
        cell: String,
        /// The refusal's own display text: the typed reason and the oracle gap that would
        /// settle it.
        message: String,
    },
}

/// The localization file stream's outcome, shaped like a row's: streamed, or refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(in crate::analysis) enum LocalizationOutcome {
    Streamed {
        files: usize,
        shadowed_files: usize,
        /// A deterministic digest over order, source, path, and exact bytes of every
        /// surviving and shadowed file — the counts' identity, as `semantic_digest` is for
        /// a registry row.
        stream_digest: String,
    },
    Refused {
        message: String,
    },
}

pub(in crate::analysis) fn records_root() -> PathBuf {
    repo_root().join("docs").join("conformance").join("parser")
}

/// Whether this invocation should write its record. Opt-in per harness — each run names its
/// own variable — because a run that rewrote the record it is checked against could not fail.
pub(in crate::analysis) fn capture_requested(variable: &str) -> bool {
    std::env::var(variable).is_ok_and(|value| !value.is_empty())
}

pub(in crate::analysis) fn environment(stellaris: BTreeMap<String, String>) -> Environment {
    Environment {
        stellaris,
        jomini: JOMINI_VERSION.to_owned(),
        rustc: rustc_version(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
    }
}

/// The installed Stellaris build, as every record's environment block states it.
pub(in crate::analysis) fn installed_build() -> BTreeMap<String, String> {
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

fn rustc_version() -> String {
    // `rust-toolchain.toml` pins the toolchain, so this string only moves on a deliberate
    // repository edit. That is what makes it worth gating on rather than merely recording —
    // the spikes could not gate on it because nothing pinned it.
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unavailable".to_owned())
}

/// One corpus's identity, computed the same way for every run: script-family files counted
/// and summed, the whole-snapshot fingerprint, and per-file digests only inside the repo.
///
/// The counts cover the script family alone — the same rule `c1` recorded under — even for a
/// run that also reads sprite and localization files, so the identity blocks stay comparable
/// across runs. The fingerprint is over the entire snapshot either way, and it is the value
/// that answers "did the source move".
pub(in crate::analysis) fn script_identity(
    corpus: &Corpus,
    snapshot: &SourceSnapshot,
) -> CorpusIdentity {
    let files: Vec<_> = snapshot
        .paths()
        .filter(|logical| snapshot.family(logical) == Some(FileFamily::Script))
        .map(|logical| {
            let bytes = snapshot
                .read(logical)
                .expect("an enumerated path reads back from its own snapshot");
            (logical.clone(), bytes)
        })
        .collect();
    CorpusIdentity {
        id: corpus.id.to_owned(),
        title: corpus.title.to_owned(),
        file_count: files.len(),
        total_bytes: files.iter().map(|(_, bytes)| bytes.len() as u64).sum(),
        fingerprint: snapshot.fingerprint().to_hex(),
        files: is_committed(&corpus.root).then(|| {
            files
                .iter()
                .map(|(logical, bytes)| {
                    (logical.as_str().to_owned(), ContentHash::of(bytes).to_hex())
                })
                .collect()
        }),
    }
}

pub(in crate::analysis) fn read(run: &str) -> Option<Manifest> {
    let bytes = fs::read(records_root().join(run).join("manifest.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Writes the artifacts, then the manifest naming exactly what reached disk.
pub(in crate::analysis) fn write(
    run: &str,
    purpose: &str,
    environment: Environment,
    corpora: Vec<CorpusRecord>,
    resolution: Option<ResolutionRecord>,
    artifacts: &[(&str, String)],
    warnings: Vec<String>,
) -> std::io::Result<PathBuf> {
    let directory = records_root().join(run);
    fs::create_dir_all(&directory)?;

    let mut hashed = BTreeMap::new();
    for (name, contents) in artifacts {
        let path = directory.join(name);
        fs::write(&path, contents)?;
        // Hashed from disk rather than from the string in hand, so the manifest describes
        // the bytes a later reader will re-hash.
        hashed.insert(
            (*name).to_owned(),
            ContentHash::of(&fs::read(&path)?).to_hex(),
        );
    }

    let manifest = Manifest {
        run: run.to_owned(),
        purpose: purpose.to_owned(),
        environment,
        corpora,
        resolution,
        artifacts: hashed,
        warnings,
    };
    let mut json = serde_json::to_string_pretty(&manifest).expect("a manifest of plain data");
    json.push('\n');
    fs::write(directory.join("manifest.json"), json)?;
    Ok(directory)
}

/// Every way this run disagrees with the record it is checked against.
///
/// Names each difference rather than returning a bare bool: "the corpus drifted" without
/// saying which corpus, or which field, sends the next person back to the diff to find out.
pub(in crate::analysis) fn drift(
    recorded: &Manifest,
    environment: &Environment,
    corpora: &[CorpusRecord],
    resolution: Option<&ResolutionRecord>,
    artifacts: &[(&str, String)],
) -> Vec<String> {
    let mut reasons = Vec::new();

    let mut compare = |field: &str, recorded: &str, observed: &str| {
        if recorded != observed {
            reasons.push(format!("{field}: recorded {recorded}, observed {observed}"));
        }
    };
    compare("jomini", &recorded.environment.jomini, &environment.jomini);
    compare("rustc", &recorded.environment.rustc, &environment.rustc);
    compare("os", &recorded.environment.os, &environment.os);
    compare("arch", &recorded.environment.arch, &environment.arch);
    for key in ["version", "rawVersion", "modsCompatibilityVersion"] {
        let absent = "absent".to_owned();
        compare(
            &format!("stellaris.{key}"),
            recorded.environment.stellaris.get(key).unwrap_or(&absent),
            environment.stellaris.get(key).unwrap_or(&absent),
        );
    }

    for observed in corpora {
        let id = &observed.identity.id;
        let Some(recorded) = recorded
            .corpora
            .iter()
            .find(|corpus| corpus.identity.id == *id)
        else {
            reasons.push(format!("corpus {id}: not in the record"));
            continue;
        };
        if recorded.identity.fingerprint != observed.identity.fingerprint {
            reasons.push(format!(
                "corpus {id}: fingerprint {} -> {} ({} files, {} bytes recorded; {} files, {} bytes observed)",
                recorded.identity.fingerprint,
                observed.identity.fingerprint,
                recorded.identity.file_count,
                recorded.identity.total_bytes,
                observed.identity.file_count,
                observed.identity.total_bytes,
            ));
        }
        // What the run *got*, not only what it read. Without this, a parser change that
        // leaves the corpus and the tools untouched — the recurrence trigger's third case —
        // passes every other comparison here.
        match (&recorded.outcome, &observed.outcome) {
            (Some(recorded), Some(outcome)) => {
                let mut count = |field: &str, recorded: usize, observed: usize| {
                    if recorded != observed {
                        reasons.push(format!("corpus {id}: {field} {recorded} -> {observed}"));
                    }
                };
                count("compared", recorded.compared, outcome.compared);
                count("agreed", recorded.agreed, outcome.agreed);
                count("diverged", recorded.diverged, outcome.diverged);
                count(
                    "tape rejections",
                    recorded.tape_rejected,
                    outcome.tape_rejected,
                );
                count(
                    "adapter recoveries",
                    recorded.adapter_recovered,
                    outcome.adapter_recovered,
                );
                count("range faults", recorded.range_faults, outcome.range_faults);
                if recorded.parsed_corpus_digest != outcome.parsed_corpus_digest {
                    reasons.push(format!(
                        "corpus {id}: parsed-corpus digest {} -> {}. The source is unchanged if no \
                         fingerprint drifted above, so this is the parser reading the same bytes \
                         differently.",
                        recorded.parsed_corpus_digest, outcome.parsed_corpus_digest
                    ));
                }
            }
            (None, None) => {}
            (Some(_), None) => reasons.push(format!(
                "corpus {id}: recorded a parse outcome, none observed"
            )),
            (None, Some(_)) => reasons.push(format!(
                "corpus {id}: observed a parse outcome, none recorded"
            )),
        }
    }
    for corpus in &recorded.corpora {
        let id = &corpus.identity.id;
        if !corpora.iter().any(|observed| observed.identity.id == *id) {
            reasons.push(format!("corpus {id}: recorded but not run"));
        }
    }

    resolution_drift(recorded.resolution.as_ref(), resolution, &mut reasons);

    // The artifacts are compared twice, for two different failures.
    //
    // Against what this run just produced. The counts above say how many, and this says
    // whether they are the *same* ones. It also covers the one thing `parsed_corpus_digest`
    // cannot: that digest is the production adapter's reading, so nothing else here would
    // notice the **second** reading changing — a Cargo.lock bump from jomini 0.35.0 to
    // 0.35.1 leaves the declared `0.35` still. `tape-rejections.txt` is where that surfaces.
    let directory = records_root().join(&recorded.run);
    for (artifact, contents) in artifacts {
        match recorded.artifacts.get(*artifact) {
            Some(digest) if ContentHash::of(contents.as_bytes()).to_hex() == *digest => {}
            Some(_) => reasons.push(format!(
                "artifact {artifact}: this run produced different content"
            )),
            None => reasons.push(format!("artifact {artifact}: not in the record")),
        }
    }
    // And against what is on disk: a record edited by hand after capture would otherwise keep
    // reporting a run that never happened.
    for (artifact, digest) in &recorded.artifacts {
        match fs::read(directory.join(artifact)) {
            Ok(bytes) if ContentHash::of(&bytes).to_hex() == *digest => {}
            Ok(_) => reasons.push(format!("artifact {artifact}: edited after capture")),
            Err(error) => reasons.push(format!("artifact {artifact}: unreadable ({error})")),
        }
    }

    reasons
}

/// The resolution half of the drift comparison: every row named, every count compared, both
/// directions — a recorded row the run no longer produces is as much drift as a new one.
fn resolution_drift(
    recorded: Option<&ResolutionRecord>,
    observed: Option<&ResolutionRecord>,
    reasons: &mut Vec<String>,
) {
    let (recorded, observed) = match (recorded, observed) {
        (Some(recorded), Some(observed)) => (recorded, observed),
        (None, None) => return,
        (Some(_), None) => {
            reasons.push("resolution: recorded but not run".to_owned());
            return;
        }
        (None, Some(_)) => {
            reasons.push("resolution: observed but not recorded".to_owned());
            return;
        }
    };

    if recorded.profile_version != observed.profile_version {
        reasons.push(format!(
            "resolution: profile version {} -> {}",
            recorded.profile_version, observed.profile_version
        ));
    }

    for observed_row in &observed.rows {
        let registry = &observed_row.registry;
        let Some(recorded_row) = recorded.rows.iter().find(|row| row.registry == *registry) else {
            reasons.push(format!("row {registry}: not in the record"));
            continue;
        };
        row_drift(
            registry,
            &recorded_row.outcome,
            &observed_row.outcome,
            reasons,
        );
    }
    for recorded_row in &recorded.rows {
        let registry = &recorded_row.registry;
        if !observed.rows.iter().any(|row| row.registry == *registry) {
            reasons.push(format!("row {registry}: recorded but not run"));
        }
    }

    if recorded.localization != observed.localization {
        reasons.push(format!(
            "localization: recorded {:?}, observed {:?}",
            recorded.localization, observed.localization
        ));
    }
}

fn row_drift(
    registry: &str,
    recorded: &RowOutcome,
    observed: &RowOutcome,
    reasons: &mut Vec<String>,
) {
    match (recorded, observed) {
        (
            RowOutcome::Resolved {
                definitions: recorded_definitions,
                removed_files: recorded_removed,
                semantic_digest: recorded_digest,
                facts: recorded_facts,
                visible_failures: recorded_failures,
            },
            RowOutcome::Resolved {
                definitions,
                removed_files,
                semantic_digest,
                facts,
                visible_failures,
            },
        ) => {
            if recorded_definitions != definitions {
                reasons.push(format!(
                    "row {registry}: definitions {recorded_definitions} -> {definitions}"
                ));
            }
            if recorded_removed != removed_files {
                reasons.push(format!(
                    "row {registry}: removed files {recorded_removed} -> {removed_files}"
                ));
            }
            count_drift(registry, "fact", recorded_facts, facts, reasons);
            count_drift(
                registry,
                "visible failure",
                recorded_failures,
                visible_failures,
                reasons,
            );
            if recorded_digest != semantic_digest {
                reasons.push(format!(
                    "row {registry}: semantic digest {recorded_digest} -> {semantic_digest}. \
                     The source is unchanged if no fingerprint drifted above, so this is the \
                     resolver producing different output from the same bytes — a winner swap \
                     or a policy change can move this while every count holds still.",
                ));
            }
        }
        (
            RowOutcome::Refused {
                cell: recorded_cell,
                message: recorded_message,
            },
            RowOutcome::Refused { cell, message },
        ) => {
            if recorded_cell != cell {
                reasons.push(format!(
                    "row {registry}: refusal cell {recorded_cell} -> {cell}"
                ));
            }
            if recorded_message != message {
                reasons.push(format!(
                    "row {registry}: refusal changed: recorded \"{recorded_message}\", \
                     observed \"{message}\""
                ));
            }
        }
        (RowOutcome::Resolved { .. }, RowOutcome::Refused { cell, message }) => {
            reasons.push(format!(
                "row {registry}: resolved when recorded, now refuses at {cell}: {message}"
            ));
        }
        (RowOutcome::Refused { cell, .. }, RowOutcome::Resolved { .. }) => {
            reasons.push(format!(
                "row {registry}: refused at {cell} when recorded, now resolves"
            ));
        }
    }
}

/// Compares two count maps over the union of their keys, so a count that disappeared is
/// named as `n -> 0` rather than silently absent from the iteration.
fn count_drift(
    registry: &str,
    what: &str,
    recorded: &BTreeMap<String, usize>,
    observed: &BTreeMap<String, usize>,
    reasons: &mut Vec<String>,
) {
    let keys: std::collections::BTreeSet<&String> =
        recorded.keys().chain(observed.keys()).collect();
    for key in keys {
        let before = recorded.get(key).copied().unwrap_or(0);
        let after = observed.get(key).copied().unwrap_or(0);
        if before != after {
            reasons.push(format!("row {registry}: {what} {key} {before} -> {after}"));
        }
    }
}

/// Whether `path` is inside the repository, which is what decides if a corpus contributes
/// per-file digests to its record.
pub(in crate::analysis) fn is_committed(path: &Path) -> bool {
    path.starts_with(repo_root())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recorded_jomini_version_matches_the_manifest() {
        // A record that named a version the harness did not link against would be worse than
        // no record: it would make a drifted comparison look like a matching one.
        let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("the crate manifest");
        assert!(
            manifest.contains(&format!("jomini = {{ version = \"{JOMINI_VERSION}\"")),
            "Cargo.toml no longer declares jomini {JOMINI_VERSION}"
        );
    }

    // Negative controls for the drift comparisons the parse-and-resolve run added. Each
    // proves the gate can go red for one failure it claims to notice, without an installed
    // corpus; the c1 comparisons were proven red against the real corpora and are documented
    // in the two harnesses' module comments.

    fn corpus(id: &str, fingerprint: &str) -> CorpusRecord {
        CorpusRecord {
            identity: CorpusIdentity {
                id: id.to_owned(),
                title: id.to_owned(),
                file_count: 2,
                total_bytes: 64,
                fingerprint: fingerprint.to_owned(),
                files: None,
            },
            outcome: None,
        }
    }

    fn resolved_row(registry: &str, failures: &[(&str, usize)]) -> RowRecord {
        RowRecord {
            registry: registry.to_owned(),
            outcome: RowOutcome::Resolved {
                definitions: 5,
                removed_files: 0,
                semantic_digest: "aa".to_owned(),
                facts: failures
                    .iter()
                    .map(|(key, count)| ((*key).to_owned(), *count))
                    .collect(),
                visible_failures: failures
                    .iter()
                    .map(|(key, count)| ((*key).to_owned(), *count))
                    .collect(),
            },
        }
    }

    fn resolution(rows: Vec<RowRecord>) -> ResolutionRecord {
        ResolutionRecord {
            profile_version: 7,
            rows,
            localization: LocalizationOutcome::Streamed {
                files: 3,
                shadowed_files: 1,
                stream_digest: "ll".to_owned(),
            },
        }
    }

    fn recorded(corpora: Vec<CorpusRecord>, resolution: Option<ResolutionRecord>) -> Manifest {
        Manifest {
            // A run name no record directory has, so the on-disk artifact comparison has
            // nothing to read and these controls exercise only the comparisons under test.
            run: "negative-control".to_owned(),
            purpose: String::new(),
            environment: environment(BTreeMap::new()),
            corpora,
            resolution,
            artifacts: BTreeMap::new(),
            warnings: Vec::new(),
        }
    }

    /// The positive control: an identical observation drifts nowhere, so every red result
    /// below is the injected difference and not a comparison that always fires.
    #[test]
    fn an_identical_observation_reports_no_drift() {
        let rows = vec![resolved_row(
            "technologies",
            &[("constants.DeclarationNeverResolves", 4)],
        )];
        let manifest = recorded(
            vec![corpus("vanilla", "aa")],
            Some(resolution(rows.clone())),
        );
        let reasons = drift(
            &manifest,
            &environment(BTreeMap::new()),
            &manifest.corpora,
            Some(&resolution(rows)),
            &[],
        );
        assert!(reasons.is_empty(), "{reasons:?}");
    }

    #[test]
    fn a_changed_corpus_fingerprint_is_named() {
        let manifest = recorded(vec![corpus("acot", "aa")], None);
        let reasons = drift(
            &manifest,
            &environment(BTreeMap::new()),
            &[corpus("acot", "bb")],
            None,
            &[],
        );
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("corpus acot")
                    && reason.contains("fingerprint aa -> bb")),
            "{reasons:?}"
        );
    }

    #[test]
    fn a_changed_visible_failure_count_is_named() {
        let manifest = recorded(
            Vec::new(),
            Some(resolution(vec![resolved_row(
                "technologies",
                &[("constants.DeclarationNeverResolves", 4)],
            )])),
        );
        let observed = resolution(vec![resolved_row(
            "technologies",
            &[("constants.DeclarationNeverResolves", 6)],
        )]);
        let reasons = drift(
            &manifest,
            &environment(BTreeMap::new()),
            &[],
            Some(&observed),
            &[],
        );
        assert!(
            reasons.iter().any(|reason| {
                reason.contains("row technologies")
                    && reason.contains("visible failure constants.DeclarationNeverResolves 4 -> 6")
            }),
            "{reasons:?}"
        );
    }

    #[test]
    fn a_changed_semantic_digest_is_named_even_when_every_count_matches() {
        // The comparison Codex's PR #22 review asked for: a winner swap changes no count, so
        // the digest must be drift-compared in its own right. The corpus-level twin — the
        // digest actually moving under a swapped winner — is
        // `resolver::conformance::the_semantic_digest_observes_a_winner_swap_the_counts_cannot`.
        let manifest = recorded(
            Vec::new(),
            Some(resolution(vec![resolved_row("technologies", &[])])),
        );
        let mut observed_row = resolved_row("technologies", &[]);
        let RowOutcome::Resolved {
            semantic_digest, ..
        } = &mut observed_row.outcome
        else {
            unreachable!("resolved_row builds a resolved outcome");
        };
        *semantic_digest = "bb".to_owned();
        let reasons = drift(
            &manifest,
            &environment(BTreeMap::new()),
            &[],
            Some(&resolution(vec![observed_row])),
            &[],
        );
        assert!(
            reasons.iter().any(|reason| {
                reason.contains("row technologies") && reason.contains("semantic digest aa -> bb")
            }),
            "{reasons:?}"
        );
    }

    #[test]
    fn a_visible_failure_that_disappears_entirely_is_still_named() {
        // The union comparison, not an iteration over the recorded side alone: a key only
        // one side has must read as `n -> 0` or `0 -> n`, never as nothing to compare.
        let manifest = recorded(
            Vec::new(),
            Some(resolution(vec![resolved_row("sprites", &[])])),
        );
        let observed = resolution(vec![resolved_row(
            "sprites",
            &[("sprite.texture.MissingTexture", 2)],
        )]);
        let reasons = drift(
            &manifest,
            &environment(BTreeMap::new()),
            &[],
            Some(&observed),
            &[],
        );
        assert!(
            reasons.iter().any(|reason| {
                reason.contains("row sprites")
                    && reason.contains("sprite.texture.MissingTexture 0 -> 2")
            }),
            "{reasons:?}"
        );
    }

    #[test]
    fn a_row_present_on_only_one_side_is_named_in_both_directions() {
        let manifest = recorded(
            Vec::new(),
            Some(resolution(vec![resolved_row("technologies", &[])])),
        );
        let observed = resolution(vec![resolved_row("events", &[])]);
        let reasons = drift(
            &manifest,
            &environment(BTreeMap::new()),
            &[],
            Some(&observed),
            &[],
        );
        assert!(
            reasons
                .iter()
                .any(|reason| reason == "row events: not in the record"),
            "{reasons:?}"
        );
        assert!(
            reasons
                .iter()
                .any(|reason| reason == "row technologies: recorded but not run"),
            "{reasons:?}"
        );
    }

    #[test]
    fn a_row_that_flips_between_resolving_and_refusing_is_named() {
        let manifest = recorded(
            Vec::new(),
            Some(resolution(vec![resolved_row("megastructures", &[])])),
        );
        let observed = resolution(vec![RowRecord {
            registry: "megastructures".to_owned(),
            outcome: RowOutcome::Refused {
                cell: "field replacement, inheritance, and defaults".to_owned(),
                message: "r8 cannot distinguish".to_owned(),
            },
        }]);
        let reasons = drift(
            &manifest,
            &environment(BTreeMap::new()),
            &[],
            Some(&observed),
            &[],
        );
        assert!(
            reasons.iter().any(|reason| {
                reason.contains("row megastructures")
                    && reason.contains("resolved when recorded, now refuses")
            }),
            "{reasons:?}"
        );
    }

    #[test]
    fn a_resolution_block_on_only_one_side_is_named() {
        let with = recorded(Vec::new(), Some(resolution(Vec::new())));
        let without = recorded(Vec::new(), None);
        let observed = resolution(Vec::new());

        let reasons = drift(&with, &environment(BTreeMap::new()), &[], None, &[]);
        assert!(
            reasons
                .iter()
                .any(|reason| reason == "resolution: recorded but not run"),
            "{reasons:?}"
        );

        let reasons = drift(
            &without,
            &environment(BTreeMap::new()),
            &[],
            Some(&observed),
            &[],
        );
        assert!(
            reasons
                .iter()
                .any(|reason| reason == "resolution: observed but not recorded"),
            "{reasons:?}"
        );
    }

    #[test]
    fn a_changed_profile_version_and_localization_outcome_are_named() {
        let manifest = recorded(Vec::new(), Some(resolution(Vec::new())));
        let observed = ResolutionRecord {
            profile_version: 8,
            rows: Vec::new(),
            localization: LocalizationOutcome::Streamed {
                files: 4,
                shadowed_files: 1,
                stream_digest: "ll".to_owned(),
            },
        };
        let reasons = drift(
            &manifest,
            &environment(BTreeMap::new()),
            &[],
            Some(&observed),
            &[],
        );
        assert!(
            reasons
                .iter()
                .any(|reason| reason == "resolution: profile version 7 -> 8"),
            "{reasons:?}"
        );
        assert!(
            reasons
                .iter()
                .any(|reason| reason.starts_with("localization: recorded")),
            "{reasons:?}"
        );
    }

    #[test]
    fn a_corpus_parse_outcome_on_only_one_side_is_named() {
        // The parse-and-resolve run records identity-only corpora. If the c1 record were
        // ever compared under a run that stopped producing outcomes — or vice versa — the
        // Option must read as drift, not as a comparison quietly skipped.
        let mut with_outcome = corpus("vanilla", "aa");
        with_outcome.outcome = Some(Outcome {
            parsed_corpus_digest: "dd".to_owned(),
            compared: 1,
            agreed: 1,
            diverged: 0,
            tape_rejected: 0,
            adapter_recovered: 0,
            range_faults: 0,
        });
        let manifest = recorded(vec![with_outcome], None);
        let reasons = drift(
            &manifest,
            &environment(BTreeMap::new()),
            &[corpus("vanilla", "aa")],
            None,
            &[],
        );
        assert!(
            reasons
                .iter()
                .any(|reason| reason == "corpus vanilla: recorded a parse outcome, none observed"),
            "{reasons:?}"
        );
    }
}
