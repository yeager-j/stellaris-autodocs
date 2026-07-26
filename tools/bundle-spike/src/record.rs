//! Captured evidence records, at parity with `docs/spikes/parser-records/` and
//! `docs/spikes/dds-records/` — with one structural difference this spike forced.
//!
//! Every earlier record in this repository is byte-comparable: re-capture from unchanged
//! inputs, get an unchanged file, and `verify` can therefore treat any difference as drift.
//! `d3-recipe` discovered what breaks that. Its first re-capture differed in two fields, both
//! wall-clock encode times, and the fix was simply to delete them — a timing figure makes
//! every re-capture differ for a reason that has nothing to do with the evidence, and
//! directional performance numbers belonged in a record of their own.
//!
//! This spike cannot take that fix, because the timings *are* the evidence. Nine of its
//! budgets are latencies. So the split is structural instead:
//!
//! - [`Manifest`] and the identity artifacts beside it — corpus digests, crate versions,
//!   bundle content hashes, byte totals, file counts, asset keys — are byte-compared by
//!   `verify`, exactly as before.
//! - Timing distributions live in `timings.json`, whose *inputs and environment* `verify`
//!   checks and whose *numbers* it never compares. They are judged against the budgets
//!   declared in the evaluation before capture, not against a previous run.
//!
//! A record that mixed the two would either lose the drift gate or lose the measurement.
//!
//! JSON is written `indent=2` with sorted keys and a trailing newline, matching
//! `tools/oracle/capture.py`, so a re-capture with unchanged inputs produces an unchanged
//! file and a diff shows only what actually moved.

use crate::corpus::{self, CorpusIdentity};
use crate::digest::{sha256, Stream};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Versions that can change a measurement without any source in this crate changing.
///
/// The two adopted adapters are named twice over: by the upstream version they wrap, and by
/// the source tree digest of the spike crate that wraps them. The version alone would not be
/// enough — these are path dependencies, so a local edit to `parser-spike/src/lexer.rs`
/// changes what this harness measured while every recorded version number stays still.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    /// From `launcher-settings.json`, never from a log banner: `game.log` records the last
    /// run rather than the installed build, and the two disagree whenever logs are stale
    /// (`docs/spikes/oracle-records/environment.md`).
    pub stellaris: BTreeMap<String, String>,
    pub jomini: String,
    pub image_dds: String,
    pub bcdec_rs: String,
    pub png: String,
    /// SHA-256 over `tools/parser-spike/src`, sorted by path.
    pub parser_spike_source: String,
    /// SHA-256 over `tools/dds-spike/src`, sorted by path.
    pub dds_spike_source: String,
    /// SHA-256 over this harness's own `src`, sorted by path.
    ///
    /// The gate every earlier spike in this repository is missing. `verify` re-hashes the
    /// artifacts a record already wrote and compares them to hashes the same run recorded, so
    /// it cannot notice that the *code* which produced them has since changed — a record can
    /// be internally consistent and describe a harness that no longer exists. Recording this
    /// makes an edit to the harness show up as drift, which is the same standard the
    /// dependency spikes are held to one line above.
    pub bundle_spike_source: String,
    pub rustc: String,
    pub os: String,
    pub arch: String,
}

pub fn environment() -> Environment {
    Environment {
        stellaris: stellaris_build(),
        jomini: parser_spike::record::JOMINI_VERSION.to_owned(),
        image_dds: dds_spike::recipe::IMAGE_DDS_VERSION.to_owned(),
        bcdec_rs: dds_spike::recipe::BCDEC_RS_VERSION.to_owned(),
        png: dds_spike::recipe::PNG_VERSION.to_owned(),
        parser_spike_source: source_tree_digest("parser-spike"),
        dds_spike_source: source_tree_digest("dds-spike"),
        bundle_spike_source: source_tree_digest("bundle-spike"),
        rustc: rustc_version(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
    }
}

/// One digest over a dependency spike's `src/`, so a local edit to an adopted adapter cannot
/// pass unnoticed behind an unchanged crate version.
fn source_tree_digest(crate_name: &str) -> String {
    let root = corpus::repo_root().join("tools").join(crate_name).join("src");
    let mut entries: BTreeMap<String, String> = BTreeMap::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            return format!("unreadable: {}", root.display());
        };
        for entry in read.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => stack.push(path),
                Ok(kind) if kind.is_file() => {
                    let Ok(bytes) = std::fs::read(&path) else {
                        continue;
                    };
                    entries.insert(corpus::logical_path(&root, &path), sha256(&bytes));
                }
                _ => {}
            }
        }
    }

    let mut stream = Stream::new();
    for (path, digest) in &entries {
        stream.push(path, digest);
    }
    stream.finish()
}

fn stellaris_build() -> BTreeMap<String, String> {
    let path = corpus::install_root().join("launcher-settings.json");
    let mut build = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(&path) else {
        build.insert("error".into(), format!("unreadable: {}", path.display()));
        return build;
    };
    let Ok(settings) = serde_json::from_str::<serde_json::Value>(&text) else {
        build.insert("error".into(), "launcher-settings.json is not JSON".into());
        return build;
    };
    for key in [
        "version",
        "rawVersion",
        "modsCompatibilityVersion",
        "distPlatform",
    ] {
        if let Some(value) = settings.get(key).and_then(|value| value.as_str()) {
            build.insert(key.to_owned(), value.to_owned());
        }
    }
    build
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub run: String,
    /// Copied verbatim from the run's binary, as `tools/oracle/capture.py` does, so the
    /// record states why the measurement exists and not only what it produced.
    pub purpose: String,
    pub environment: Environment,
    pub corpora: Vec<CorpusIdentity>,
    /// Artifact file name to SHA-256, for artifacts whose bytes must be reproducible.
    pub artifacts: BTreeMap<String, String>,
    /// Artifacts deliberately excluded from byte comparison, and why.
    ///
    /// Only timing distributions belong here. Naming them in the manifest rather than
    /// omitting them keeps the exclusion visible: a reader can see that the record chose not
    /// to compare a file, instead of discovering later that the gate silently never looked.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub uncompared_artifacts: BTreeMap<String, String>,
    /// Empty when the run is clean. A warning never silently downgrades to a footnote.
    pub warnings: Vec<String>,
}

pub fn records_root() -> PathBuf {
    corpus::repo_root()
        .join("docs")
        .join("spikes")
        .join("bundle-records")
}

/// One artifact of a record.
pub struct Artifact {
    pub name: String,
    pub contents: String,
    /// `false` only for timing distributions. See the module documentation.
    pub compared: bool,
}

impl Artifact {
    pub fn identity(name: impl Into<String>, contents: impl Into<String>) -> Self {
        Artifact {
            name: name.into(),
            contents: contents.into(),
            compared: true,
        }
    }

    pub fn timings(name: impl Into<String>, contents: impl Into<String>) -> Self {
        Artifact {
            name: name.into(),
            contents: contents.into(),
            compared: false,
        }
    }
}

/// Write one record directory: its artifacts, then the manifest that hashes them.
///
/// The manifest is written last and hashes what is already on disk, so it can never name an
/// artifact that was not produced.
pub fn write(
    run: &str,
    purpose: &str,
    corpora: Vec<CorpusIdentity>,
    artifacts: Vec<Artifact>,
    warnings: Vec<String>,
) -> std::io::Result<PathBuf> {
    let directory = records_root().join(run);
    std::fs::create_dir_all(&directory)?;

    let mut hashes = BTreeMap::new();
    let mut uncompared = BTreeMap::new();
    for artifact in &artifacts {
        std::fs::write(directory.join(&artifact.name), &artifact.contents)?;
        if artifact.compared {
            hashes.insert(artifact.name.clone(), sha256(artifact.contents.as_bytes()));
        } else {
            uncompared.insert(
                artifact.name.clone(),
                "timing distribution: judged against declared budgets, not against a prior \
                 capture"
                    .to_owned(),
            );
        }
    }

    let manifest = Manifest {
        run: run.to_owned(),
        purpose: purpose.to_owned(),
        environment: environment(),
        corpora,
        artifacts: hashes,
        uncompared_artifacts: uncompared,
        warnings,
    };
    write_json(&directory.join("manifest.json"), &manifest)?;
    Ok(directory)
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    std::fs::write(path, text)
}

pub fn to_json<T: Serialize>(value: &T) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("record values are serializable");
    text.push('\n');
    text
}

/// Compact JSON, for bundle payloads and for the denominator they are compared against.
///
/// Not a micro-optimization; a correctness fix for the measurement. Pretty-printed JSON pays
/// two spaces per level of nesting, and the canonical unsharded payload nests every record
/// several levels deeper than a per-document file does. Comparing a pretty-printed
/// single-document payload against pretty-printed per-document files measures indentation
/// depth as if it were materialization overhead — the first capture of `b2-shape` reported an
/// in-bundle ratio below 1.0 for exactly that reason, which would have meant a bundle smaller
/// than the model it contains.
///
/// `docs/technical-design.md:632` asks for a human-readable *manifest*. It does not ask for
/// human-readable documentation records, and production would not ship them that way.
pub fn to_compact_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("bundle values are serializable")
}

pub fn read_manifest(path: &Path) -> std::io::Result<Manifest> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(std::io::Error::from)
}
