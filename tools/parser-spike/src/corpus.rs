//! Corpus locations, enumeration, and content identity.
//!
//! Mirrors the conventions `tools/oracle/oracle_paths.py` established for the resolver
//! harness: every root is overridable by environment variable so the harness can run
//! against another installation without editing code, and enumeration sorts by normalized
//! path bytes rather than filesystem walk order, so a result never depends on how a
//! directory happened to be traversed (`docs/technical-design.md:315`).
//!
//! No corpus content is ever copied into the repository. Records hold logical paths and
//! SHA-256 digests, which is what a licensed local installation needs to reproduce a run —
//! the same rule `fixtures/oracle/README.md` states for the oracle fixtures.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Extensions parsed as Clausewitz script.
///
/// `.yml` is excluded on purpose. Stellaris localization is a different language with its
/// own owner (`docs/technical-design.md:345`); feeding it to a Clausewitz parser would
/// manufacture failures that say nothing about either module. Its encodings are still
/// exercised, through fixtures, because a UTF-8 BOM reaches script files too.
pub const SCRIPT_EXTENSIONS: &[&str] = &["txt", "asset", "gfx", "gui", "mod"];

/// Top-level directories that hold Clausewitz script.
///
/// An allowlist rather than a denylist, because the game install also contains `licenses/`,
/// `pdx_launcher/`, `sound/`, and an application bundle, all of which contain `.txt` files
/// that were never script. Counting a font licence as a parser failure would inflate the
/// failure rate with a number that says nothing about the parser.
///
/// The list is deliberately not a curation of *parseable* files: `common/` and `interface/`
/// still contain prose such as `common/HOW_TO_MAKE_NEW_SHIPS.txt` and `interface/credits.txt`,
/// which live in script directories and are not script. Those stay in, because how the
/// parser treats them is a real question about failure isolation rather than a nuisance.
pub const SCRIPT_DIRECTORIES: &[&str] = &[
    "common",
    "events",
    "interface",
    "gfx",
    "map",
    "prescripted_countries",
];

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
    // The crate lives at <repo>/tools/parser-spike.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is nested two levels below the repository root")
        .to_path_buf()
}

pub fn fixtures_root() -> PathBuf {
    repo_root().join("fixtures").join("parser")
}

/// One named body of source the spike parses as a unit.
#[derive(Debug, Clone)]
pub struct Corpus {
    pub id: String,
    /// Human-readable origin, recorded so a record names what it measured.
    pub title: String,
    pub root: PathBuf,
}

/// The pinned corpora, in the order records report them.
///
/// Workshop mods are identified by their numeric Workshop id rather than by title: a title
/// is editable metadata, while the id is what the filesystem and `dlc_load.json` address.
pub fn default_corpora() -> Vec<Corpus> {
    let workshop = workshop_root();
    vec![
        Corpus {
            id: "vanilla".into(),
            title: "Stellaris base game".into(),
            root: install_root(),
        },
        Corpus {
            id: "giga".into(),
            title: "Gigastructural Engineering & More (1121692237)".into(),
            root: workshop.join("1121692237"),
        },
        Corpus {
            id: "acot".into(),
            title: "Ancient Cache of Technologies (1419304439)".into(),
            root: workshop.join("1419304439"),
        },
        Corpus {
            id: "aot".into(),
            title: "Acquisition of Technology (2178603631)".into(),
            root: workshop.join("2178603631"),
        },
        Corpus {
            id: "fixtures".into(),
            title: "Hand-authored parser fixtures (valid)".into(),
            root: fixtures_root().join("valid"),
        },
    ]
}

/// One file selected for parsing.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Path relative to the corpus root, `/`-separated.
    pub logical: String,
    pub absolute: PathBuf,
    pub bytes: u64,
}

/// Every script file under `root`, ordered by normalized logical-path bytes.
///
/// Only [`SCRIPT_DIRECTORIES`] are descended into, plus root-level `.mod` descriptors. `dlc/`
/// is excluded with everything else: DLC archives supply visual assets, not a script layer
/// (`docs/technical-design.md:279`).
pub fn enumerate(root: &Path) -> std::io::Result<Vec<SourceFile>> {
    let mut files = Vec::new();
    let mut stack: Vec<PathBuf> = SCRIPT_DIRECTORIES
        .iter()
        .map(|name| root.join(name))
        .filter(|path| path.is_dir())
        .collect();

    for entry in std::fs::read_dir(root)?.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|extension| extension == "mod") {
            files.push(SourceFile {
                logical: logical_path(root, &path),
                bytes: entry.metadata().map(|meta| meta.len()).unwrap_or(0),
                absolute: path,
            });
        }
    }

    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            // An unreadable subdirectory is reported once, not fatal: the spike measures
            // parsing, and a permissions failure elsewhere must not erase the whole run.
            Err(error) => {
                eprintln!("warning: cannot read {}: {error}", dir.display());
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(kind) => kind,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && has_script_extension(&path) {
                let logical = logical_path(root, &path);
                let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                files.push(SourceFile {
                    logical,
                    absolute: path,
                    bytes,
                });
            }
        }
    }

    files.sort_by(|a, b| a.logical.as_bytes().cmp(b.logical.as_bytes()));
    Ok(files)
}

fn has_script_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| SCRIPT_EXTENSIONS.contains(&ext))
}

fn logical_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// What a record stores about a corpus: enough to prove the same input, never the input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusIdentity {
    pub id: String,
    pub title: String,
    pub file_count: usize,
    pub total_bytes: u64,
    /// SHA-256 over the sorted `<logical path>\0<file digest>\n` stream. One value that
    /// changes if any path or any byte changes.
    pub tree_digest: String,
    /// Per-file digests, recorded only for corpora committed to this repository.
    ///
    /// The game corpora run to about 8,000 files. Listing them in every record would add a
    /// megabyte of duplicated JSON per run for one marginal gain — naming which file moved —
    /// that re-running the coverage tool supplies anyway. The tree digest is what the drift
    /// gate compares, and it is unaffected.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, String>,
}

pub fn identify(corpus: &Corpus, files: &[SourceFile]) -> std::io::Result<CorpusIdentity> {
    let mut entries = BTreeMap::new();
    let mut total_bytes = 0u64;
    for file in files {
        let bytes = std::fs::read(&file.absolute)?;
        total_bytes += bytes.len() as u64;
        entries.insert(file.logical.clone(), sha256(&bytes));
    }

    let mut stream = Sha256::new();
    for (logical, digest) in &entries {
        stream.update(logical.as_bytes());
        stream.update([0]);
        stream.update(digest.as_bytes());
        stream.update([b'\n']);
    }
    let tree_digest = stream
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    let committed = corpus.root.starts_with(repo_root());
    Ok(CorpusIdentity {
        id: corpus.id.clone(),
        title: corpus.title.clone(),
        file_count: entries.len(),
        total_bytes,
        tree_digest,
        files: if committed { entries } else { BTreeMap::new() },
    })
}
