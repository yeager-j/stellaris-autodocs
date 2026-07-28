//! The captured run record, and the drift comparison that makes it a gate.
//!
//! A record is what lets a later run say "the same corpus, read by the same code, still
//! produces the same answer" — and, when it does not, name which of those three moved. It
//! holds identities and counts only: **no corpus content is ever copied into the
//! repository.** Logical paths and digests are what a licensed local installation needs to
//! reproduce a run, and they are all a record carries for a corpus outside the repo.
//!
//! The manifest is written last, hashing the artifacts already on disk, so a manifest can
//! never name a file that was not produced. The shape follows the spike records under
//! `docs/spikes/*-records/` deliberately: Phase 4M extends this format with resolve results
//! rather than inventing a second one.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::source::fingerprint::ContentHash;

/// The Jomini requirement this harness links against.
///
/// Duplicated from `Cargo.toml` rather than read from it, so a record states the version the
/// code was compiled with; `the_recorded_jomini_version_matches_the_manifest` keeps the two
/// honest.
pub(super) const JOMINI_VERSION: &str = "0.35";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Manifest {
    pub(super) run: String,
    pub(super) purpose: String,
    pub(super) environment: Environment,
    pub(super) corpora: Vec<CorpusIdentity>,
    pub(super) artifacts: BTreeMap<String, String>,
    pub(super) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Environment {
    /// From `launcher-settings.json`, never from `game.log`: the log banner records the last
    /// run rather than the installed build, and the two disagree whenever logs are stale.
    pub(super) stellaris: BTreeMap<String, String>,
    pub(super) jomini: String,
    pub(super) rustc: String,
    pub(super) os: String,
    pub(super) arch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct CorpusIdentity {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) file_count: usize,
    pub(super) total_bytes: u64,
    /// The production `/v3` source fingerprint — the same identity a build derives, so a
    /// corpus that drifts here is a corpus that would change a revision.
    pub(super) fingerprint: String,
    /// Per-file digests, for corpora inside the repository only. A licensed local
    /// installation is identified by its fingerprint and counts; enumerating its paths would
    /// put a shipped product's file listing in this repository for no verification gain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) files: Option<BTreeMap<String, String>>,
}

pub(super) fn records_root() -> PathBuf {
    super::repo_root()
        .join("docs")
        .join("conformance")
        .join("parser")
}

/// Whether this invocation should write its record. Opt-in, because a run that rewrote the
/// record it is checked against could not fail.
pub(super) fn capture_requested() -> bool {
    std::env::var("PARSER_CONFORMANCE_CAPTURE").is_ok_and(|value| !value.is_empty())
}

pub(super) fn environment(stellaris: BTreeMap<String, String>) -> Environment {
    Environment {
        stellaris,
        jomini: JOMINI_VERSION.to_owned(),
        rustc: rustc_version(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
    }
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

pub(super) fn read(run: &str) -> Option<Manifest> {
    let bytes = fs::read(records_root().join(run).join("manifest.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Writes the artifacts, then the manifest naming exactly what reached disk.
pub(super) fn write(
    run: &str,
    purpose: &str,
    environment: Environment,
    corpora: Vec<CorpusIdentity>,
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
pub(super) fn drift(
    recorded: &Manifest,
    environment: &Environment,
    corpora: &[CorpusIdentity],
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
        let Some(recorded) = recorded
            .corpora
            .iter()
            .find(|corpus| corpus.id == observed.id)
        else {
            reasons.push(format!("corpus {}: not in the record", observed.id));
            continue;
        };
        if recorded.fingerprint != observed.fingerprint {
            reasons.push(format!(
                "corpus {}: fingerprint {} -> {} ({} files, {} bytes recorded; {} files, {} bytes observed)",
                observed.id,
                recorded.fingerprint,
                observed.fingerprint,
                recorded.file_count,
                recorded.total_bytes,
                observed.file_count,
                observed.total_bytes,
            ));
        }
    }
    for corpus in &recorded.corpora {
        if !corpora.iter().any(|observed| observed.id == corpus.id) {
            reasons.push(format!("corpus {}: recorded but not run", corpus.id));
        }
    }

    // The artifacts are the evidence the manifest speaks for. Re-hashing them catches the
    // case the corpus and environment comparisons cannot: a record edited by hand after
    // capture, which would otherwise keep reporting a run that never happened.
    let directory = records_root().join(&recorded.run);
    for (artifact, digest) in &recorded.artifacts {
        match fs::read(directory.join(artifact)) {
            Ok(bytes) if ContentHash::of(&bytes).to_hex() == *digest => {}
            Ok(_) => reasons.push(format!("artifact {artifact}: edited after capture")),
            Err(error) => reasons.push(format!("artifact {artifact}: unreadable ({error})")),
        }
    }

    reasons
}

/// Whether `path` is inside the repository, which is what decides if a corpus contributes
/// per-file digests to its record.
pub(super) fn is_committed(path: &Path) -> bool {
    path.starts_with(super::repo_root())
}

#[test]
fn the_recorded_jomini_version_matches_the_manifest() {
    // A record that named a version the harness did not link against would be worse than no
    // record: it would make a drifted comparison look like a matching one.
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("the crate manifest");
    assert!(
        manifest.contains(&format!("jomini = {{ version = \"{JOMINI_VERSION}\"")),
        "Cargo.toml no longer declares jomini {JOMINI_VERSION}"
    );
}
