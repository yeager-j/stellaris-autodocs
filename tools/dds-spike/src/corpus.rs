//! Corpus locations, enumeration, and content identity.
//!
//! Follows the conventions `tools/oracle/oracle_paths.py` established and
//! `tools/parser-spike/src/corpus.rs` carried into Rust: every root is overridable by environment
//! variable so the harness can run against another installation without editing code, and
//! enumeration sorts by normalized path bytes rather than filesystem walk order, so a result
//! never depends on how a directory happened to be traversed
//! (`docs/technical-design.md:315`).
//!
//! No corpus content is ever copied into the repository. Records hold logical paths and SHA-256
//! digests, which is what a licensed local installation needs to reproduce a run.

use crate::digest::{sha256, Stream};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Unlike the parser spike's script extensions, this needs no directory allowlist.
///
/// `.dds` is unambiguous where `.txt` was not: the install has no font licences or readmes
/// carrying this extension. Enumerating the whole root is also the point — the two files in
/// vanilla that carry the extension without the format are a result, and a curated list of
/// texture directories would have excluded exactly the surprises worth finding.
pub const TEXTURE_EXTENSION: &str = "dds";

fn env_path(name: &str, default: &str) -> PathBuf {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => PathBuf::from(value),
        _ => expand_home(default),
    }
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var("HOME") {
            Ok(home) => PathBuf::from(home).join(rest),
            Err(_) => PathBuf::from(path),
        },
        None => PathBuf::from(path),
    }
}

pub fn install_root() -> PathBuf {
    env_path(
        "STELLARIS_INSTALL_ROOT",
        "~/Library/Application Support/Steam/steamapps/common/Stellaris",
    )
}

pub fn workshop_root() -> PathBuf {
    env_path(
        "STELLARIS_WORKSHOP_ROOT",
        "~/Library/Application Support/Steam/steamapps/workshop/content/281990",
    )
}

pub fn repo_root() -> PathBuf {
    // The crate lives at <repo>/tools/dds-spike.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is nested two levels below the repository root")
        .to_path_buf()
}

pub fn fixtures_root() -> PathBuf {
    repo_root().join("fixtures").join("assets").join("dds")
}

/// One named body of textures the spike measures as a unit.
#[derive(Debug, Clone)]
pub struct Corpus {
    pub id: String,
    pub title: String,
    pub root: PathBuf,
}

/// The pinned corpora, in the order records report them.
pub fn default_corpora() -> Vec<Corpus> {
    vec![
        Corpus {
            id: "vanilla".into(),
            title: "Stellaris base game".into(),
            root: install_root(),
        },
        Corpus {
            id: "workshop".into(),
            title: "Installed Workshop mods (281990)".into(),
            root: workshop_root(),
        },
        Corpus {
            id: "fixtures".into(),
            title: "Hand-generated DDS fixtures".into(),
            root: fixtures_root(),
        },
    ]
}

/// One file selected for measurement.
#[derive(Debug, Clone)]
pub struct TextureFile {
    /// Path relative to the corpus root, `/`-separated.
    pub logical: String,
    pub absolute: PathBuf,
    pub bytes: u64,
}

/// Every `.dds` under `root`, ordered by normalized logical-path bytes.
pub fn enumerate(root: &Path) -> std::io::Result<Vec<TextureFile>> {
    let mut files = Vec::new();
    if !root.is_dir() {
        return Ok(files);
    }
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            // Reported once, not fatal: the spike measures decoding, and a permissions failure
            // elsewhere must not erase the whole run.
            Err(error) => {
                eprintln!("warning: cannot read {}: {error}", dir.display());
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && has_texture_extension(&path) {
                files.push(TextureFile {
                    logical: logical_path(root, &path),
                    bytes: entry.metadata().map(|meta| meta.len()).unwrap_or(0),
                    absolute: path,
                });
            }
        }
    }

    files.sort_by(|a, b| a.logical.as_bytes().cmp(b.logical.as_bytes()));
    Ok(files)
}

fn has_texture_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(TEXTURE_EXTENSION))
}

pub fn logical_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// What a record stores about a corpus: enough to prove the same input, never the input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusIdentity {
    pub id: String,
    pub title: String,
    pub file_count: usize,
    pub total_bytes: u64,
    /// SHA-256 over the sorted `<logical path>\0<file digest>\n` stream.
    pub tree_digest: String,
    /// Per-file digests, recorded only for corpora committed to this repository.
    ///
    /// This matters more here than it did for the parser spike: the fixture corpus is binary, so
    /// per-file digests are the only committed integrity check a reader can apply to it without
    /// running the generator.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, String>,
}

pub fn identify(corpus: &Corpus, files: &[TextureFile]) -> std::io::Result<CorpusIdentity> {
    let mut entries = BTreeMap::new();
    let mut total_bytes = 0u64;
    for file in files {
        let bytes = std::fs::read(&file.absolute)?;
        total_bytes += bytes.len() as u64;
        entries.insert(file.logical.clone(), sha256(&bytes));
    }

    let mut stream = Stream::new();
    for (logical, digest) in &entries {
        stream.push(logical, digest);
    }

    let committed = corpus.root.starts_with(repo_root());
    Ok(CorpusIdentity {
        id: corpus.id.clone(),
        title: corpus.title.clone(),
        file_count: entries.len(),
        total_bytes,
        tree_digest: stream.finish(),
        files: if committed { entries } else { BTreeMap::new() },
    })
}
