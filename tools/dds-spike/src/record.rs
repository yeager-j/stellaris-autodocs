//! Captured evidence records, at parity with `docs/spikes/parser-records/`.
//!
//! A record proves which inputs produced which numbers. It pins the corpus by content digest, the
//! decoders and encoders by version, and every emitted artifact by hash, so `verify` can later
//! tell a stale record from a current one instead of leaving a reader to assume the file still
//! describes this machine.
//!
//! JSON is written `indent=2` with sorted keys and a trailing newline, matching
//! `tools/oracle/capture.py`, so a re-capture with unchanged inputs produces an unchanged file and
//! a diff shows only what actually moved.

use crate::corpus::{self, CorpusIdentity};
use crate::digest::sha256;
use crate::recipe;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Versions that can change a measurement without any source changing.
///
/// `bcdec_rs` is listed beside `image_dds` because it is what actually decodes the compressed
/// classes: a bump there changes output pixels with no first-party change and no change to
/// `image_dds`'s own version. A record that named only the direct dependency could be current and
/// wrong at the same time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    /// From `launcher-settings.json`, never from a log banner: `game.log` records the last run
    /// rather than the installed build, and the two disagree whenever logs are stale
    /// (`docs/spikes/oracle-records/environment.md`).
    pub stellaris: BTreeMap<String, String>,
    pub image_dds: String,
    pub bcdec_rs: String,
    pub texture2ddecoder: String,
    pub png: String,
    pub image_webp: String,
    pub rustc: String,
    pub os: String,
    pub arch: String,
}

pub fn environment() -> Environment {
    Environment {
        stellaris: stellaris_build(),
        image_dds: recipe::IMAGE_DDS_VERSION.to_owned(),
        bcdec_rs: recipe::BCDEC_RS_VERSION.to_owned(),
        texture2ddecoder: recipe::TEXTURE2DDECODER_VERSION.to_owned(),
        png: recipe::PNG_VERSION.to_owned(),
        image_webp: recipe::IMAGE_WEBP_VERSION.to_owned(),
        rustc: rustc_version(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
    }
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
    /// Copied verbatim from the binary's `PURPOSE`, as `tools/oracle/capture.py` does, so the
    /// record states why the measurement exists and not only what it produced.
    pub purpose: String,
    pub environment: Environment,
    pub corpora: Vec<CorpusIdentity>,
    /// Artifact file name to SHA-256.
    pub artifacts: BTreeMap<String, String>,
    /// Empty when the run is clean. A warning never silently downgrades to a footnote.
    pub warnings: Vec<String>,
}

pub fn records_root() -> PathBuf {
    corpus::repo_root()
        .join("docs")
        .join("spikes")
        .join("dds-records")
}

/// Write one record directory: its artifacts, then the manifest that hashes them.
///
/// The manifest is written last and hashes what is already on disk, so a manifest can never name
/// an artifact that was not produced.
pub fn write(
    run: &str,
    purpose: &str,
    corpora: Vec<CorpusIdentity>,
    artifacts: Vec<(String, String)>,
    warnings: Vec<String>,
) -> std::io::Result<PathBuf> {
    let directory = records_root().join(run);
    std::fs::create_dir_all(&directory)?;

    let mut hashes = BTreeMap::new();
    for (name, contents) in &artifacts {
        std::fs::write(directory.join(name), contents)?;
        hashes.insert(name.clone(), sha256(contents.as_bytes()));
    }

    let manifest = Manifest {
        run: run.to_owned(),
        purpose: purpose.to_owned(),
        environment: environment(),
        corpora,
        artifacts: hashes,
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

pub fn read_manifest(path: &Path) -> std::io::Result<Manifest> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(std::io::Error::from)
}

/// Whether a capture was requested, by the flag every capture binary in this repository uses.
pub fn capture_requested() -> bool {
    std::env::args().any(|argument| argument == "--capture")
}

/// The line printed when a run produced numbers but was not asked to keep them.
pub const NOT_CAPTURED: &str = "(not captured; pass --capture to write the record)";
