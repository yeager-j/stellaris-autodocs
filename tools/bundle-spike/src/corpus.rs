//! Revision cases, source snapshots, and content identity.
//!
//! The unit here is a **revision case**: one Vanilla contributor plus at most one Target Mod,
//! which is exactly the two-contributor scope `docs/technical-design.md:283` gives the MVP
//! resolver. That is why this module exists rather than reusing `parser_spike::corpus` or
//! `dds_spike::corpus` wholesale — those measure one tree at a time, and nothing in a
//! single-tree corpus can express "resolved together".
//!
//! Script enumeration delegates to `parser_spike::corpus::enumerate`, so the files this
//! spike parses are the same files the parser spike measured. Localization is enumerated
//! here, because the parser spike deliberately excluded `.yml` — it is a different language
//! with its own module owner (`docs/technical-design.md:349`) and feeding it to a Clausewitz
//! parser would have manufactured failures. This spike needs it anyway: preserved
//! all-language localization is one of the two things the bundle format is being measured
//! against.
//!
//! Roots are environment-overridable exactly as the other harnesses' are, and no corpus
//! content is ever copied into the repository.

use crate::digest::{sha256, Stream};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Localization lives in one directory with a per-language subdirectory, plus the `replace/`
/// subdirectory whose files win from any position in the stream
/// (`docs/spikes/resolver-evaluation.md:106`).
pub const LOCALISATION_DIR: &str = "localisation";
pub const LOCALISATION_EXTENSION: &str = "yml";

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
    // The crate lives at <repo>/tools/bundle-spike.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is nested two levels below the repository root")
        .to_path_buf()
}

/// Where this spike's own committed fixtures live.
pub fn fixtures_root() -> PathBuf {
    repo_root().join("fixtures").join("bundle")
}

/// The malformed-source golden fixture.
///
/// This spike's own, not `fixtures/parser/malformed/`. Those files are flat `.txt` at the
/// fixture root, which is the right shape for asking what a parser does with a broken file
/// and the wrong shape for asking what a bundle does: nothing in them sits on a registry
/// path, so nothing in them can make a registry's entry set incomplete. The fault shapes are
/// restated there rather than referenced, for the reason `fixtures/parser/` gives about the
/// oracle fixtures — a fixture frozen against one spike's evidence should not silently become
/// a dependency of another's.
pub fn malformed_fixtures_root() -> PathBuf {
    fixtures_root().join("malformed")
}

/// The technology-redefinition oracle fixture, owned by the resolver spike and read
/// unmodified. Its checksums are pinned into every captured oracle record, so a change here
/// would break that spike's evidence as well as this one's.
pub fn oracle_target_root() -> PathBuf {
    repo_root().join("fixtures").join("oracle").join("target")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributorKind {
    Vanilla,
    TargetMod,
}

/// One source of content within a revision case.
#[derive(Debug, Clone)]
pub struct Contributor {
    pub id: String,
    pub title: String,
    pub root: PathBuf,
    pub kind: ContributorKind,
}

/// One Vanilla contributor plus at most one Target Mod, resolved and documented together.
///
/// `target: None` is not a degenerate case. A Vanilla-only revision is what the product
/// builds before any mod is selected, it is the largest single body of content in the
/// corpus, and it is the shared substrate whose localization every other revision duplicates
/// — which is what makes cross-revision deduplication measurable at all.
#[derive(Debug, Clone)]
pub struct RevisionCase {
    pub id: String,
    pub title: String,
    pub vanilla: Contributor,
    pub target: Option<Contributor>,
}

impl RevisionCase {
    pub fn contributors(&self) -> Vec<&Contributor> {
        match &self.target {
            Some(target) => vec![&self.vanilla, target],
            None => vec![&self.vanilla],
        }
    }
}

fn vanilla_contributor() -> Contributor {
    Contributor {
        id: "vanilla".into(),
        title: "Stellaris base game".into(),
        root: install_root(),
        kind: ContributorKind::Vanilla,
    }
}

fn workshop_contributor(id: &str, title: &str, workshop_id: &str) -> Contributor {
    Contributor {
        id: id.into(),
        title: title.into(),
        root: workshop_root().join(workshop_id),
        kind: ContributorKind::TargetMod,
    }
}

fn fixture_contributor(id: &str, title: &str, root: PathBuf) -> Contributor {
    Contributor {
        id: id.into(),
        title: title.into(),
        root,
        kind: ContributorKind::TargetMod,
    }
}

/// The pinned revision cases, in the order records report them.
///
/// The first four are the corpus the spike names. The last two are the golden fixtures it
/// names: the malformed source that must publish Incomplete Documentation rather than a
/// partial bundle, and the technology redefinition whose expected effective fields the
/// resolver spike already established against the game.
pub fn default_cases() -> Vec<RevisionCase> {
    let vanilla = vanilla_contributor();
    vec![
        RevisionCase {
            id: "vanilla".into(),
            title: "Vanilla Content only".into(),
            vanilla: vanilla.clone(),
            target: None,
        },
        RevisionCase {
            id: "acot".into(),
            title: "Ancient Cache of Technologies (1419304439)".into(),
            vanilla: vanilla.clone(),
            target: Some(workshop_contributor(
                "acot",
                "Ancient Cache of Technologies (1419304439)",
                "1419304439",
            )),
        },
        RevisionCase {
            id: "giga".into(),
            title: "Gigastructural Engineering & More (1121692237)".into(),
            vanilla: vanilla.clone(),
            target: Some(workshop_contributor(
                "giga",
                "Gigastructural Engineering & More (1121692237)",
                "1121692237",
            )),
        },
        RevisionCase {
            id: "aot".into(),
            title: "Acquisition of Technology (2178603631)".into(),
            vanilla: vanilla.clone(),
            target: Some(workshop_contributor(
                "aot",
                "Acquisition of Technology (2178603631)",
                "2178603631",
            )),
        },
        RevisionCase {
            id: "malformed".into(),
            title: "Malformed-source golden fixture".into(),
            vanilla: vanilla.clone(),
            target: Some(fixture_contributor(
                "malformed",
                "fixtures/bundle/malformed",
                malformed_fixtures_root(),
            )),
        },
        RevisionCase {
            id: "redefinition".into(),
            title: "Technology-redefinition oracle fixture".into(),
            vanilla,
            target: Some(fixture_contributor(
                "redefinition",
                "fixtures/oracle/target",
                oracle_target_root(),
            )),
        },
    ]
}

/// One file presented for analysis.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Path relative to the contributor root, `/`-separated.
    pub logical: String,
    pub absolute: PathBuf,
    pub bytes: u64,
}

/// What one contributor presents to analysis.
///
/// Paths and sizes only. Bytes are read by the phase that needs them, because the build
/// phases are what this spike times and a snapshot that pre-read everything would move the
/// filesystem cost out of the phase that pays it in production. The correctness-first
/// protocol at `docs/technical-design.md:414` reads and hashes twice on purpose; imitating
/// that is the point, not an inefficiency to optimize away.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub contributor: String,
    pub kind: ContributorKind,
    pub root: PathBuf,
    pub script: Vec<SourceFile>,
    pub localisation: Vec<SourceFile>,
    /// `replace_path` declarations from the mod descriptor, normalized without a trailing
    /// separator. Empty for Vanilla, which never declares one.
    pub replace_paths: Vec<String>,
}

impl Snapshot {
    pub fn file_count(&self) -> usize {
        self.script.len() + self.localisation.len()
    }

    pub fn total_bytes(&self) -> u64 {
        self.script.iter().map(|file| file.bytes).sum::<u64>()
            + self.localisation.iter().map(|file| file.bytes).sum::<u64>()
    }
}

pub fn snapshot(contributor: &Contributor) -> std::io::Result<Snapshot> {
    let script = parser_spike::corpus::enumerate(&contributor.root)?
        .into_iter()
        .map(|file| SourceFile {
            logical: file.logical,
            absolute: file.absolute,
            bytes: file.bytes,
        })
        .collect();

    Ok(Snapshot {
        contributor: contributor.id.clone(),
        kind: contributor.kind,
        root: contributor.root.clone(),
        script,
        localisation: enumerate_localisation(&contributor.root)?,
        replace_paths: replace_paths(&contributor.root),
    })
}

/// Every `.yml` under `localisation/`, ordered by normalized logical-path bytes.
///
/// The order matters and is not incidental: localization resolves through its own ordered
/// stream — surviving Vanilla files, then mod files in enabled-mod order, then every
/// `replace/` file — and within one phase the last loaded key wins. Traversal order must
/// never contribute to that, so it is sorted here and the `replace/` phase is separated by
/// the resolver rather than by where a directory walk happened to arrive.
pub fn enumerate_localisation(root: &Path) -> std::io::Result<Vec<SourceFile>> {
    let mut files = Vec::new();
    let start = root.join(LOCALISATION_DIR);
    if !start.is_dir() {
        return Ok(files);
    }

    let mut stack = vec![start];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
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
            } else if file_type.is_file() && has_extension(&path, LOCALISATION_EXTENSION) {
                files.push(SourceFile {
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

/// `replace_path` declarations read from the mod's own descriptor.
///
/// A missing or unreadable descriptor yields no declarations rather than an error. Vanilla
/// has none, and the committed fixtures are directories rather than installed mods; treating
/// their absence as a failure would make the fixture cases unbuildable for a reason unrelated
/// to what they test.
pub fn replace_paths(root: &Path) -> Vec<String> {
    let descriptor = root.join("descriptor.mod");
    let Ok(text) = std::fs::read_to_string(&descriptor) else {
        return Vec::new();
    };

    let mut declared = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("replace_path") else {
            continue;
        };
        let Some(value) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        if !value.is_empty() {
            declared.push(value.trim_end_matches('/').to_owned());
        }
    }
    declared.sort();
    declared.dedup();
    declared
}

fn has_extension(path: &Path, wanted: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(wanted))
}

pub fn logical_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// What a record stores about one contributor: enough to prove the same input, never the
/// input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusIdentity {
    pub id: String,
    pub title: String,
    pub script_files: usize,
    pub localisation_files: usize,
    pub total_bytes: u64,
    /// SHA-256 over the sorted `<logical path>\0<file digest>\n` stream, across script and
    /// localization together. One value that changes if any path or any byte changes.
    pub tree_digest: String,
    /// Per-file digests, recorded only for corpora committed to this repository.
    ///
    /// The game corpora run to thousands of files; listing them in every record would add a
    /// megabyte of duplicated JSON per run to name which file moved, which re-running the
    /// tool supplies anyway. Committed fixtures are listed, because for those the digest is
    /// the only integrity check a reader can apply without regenerating them — and because
    /// `d4-failures` reported `ok` against an altered fixture whose corpus its manifest had
    /// simply failed to name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, String>,
}

pub fn identify(contributor: &Contributor, snapshot: &Snapshot) -> std::io::Result<CorpusIdentity> {
    let mut entries = BTreeMap::new();
    let mut total_bytes = 0u64;
    for file in snapshot.script.iter().chain(snapshot.localisation.iter()) {
        let bytes = std::fs::read(&file.absolute)?;
        total_bytes += bytes.len() as u64;
        entries.insert(file.logical.clone(), sha256(&bytes));
    }

    let mut stream = Stream::new();
    for (logical, digest) in &entries {
        stream.push(logical, digest);
    }

    let committed = contributor.root.starts_with(repo_root());
    Ok(CorpusIdentity {
        id: contributor.id.clone(),
        title: contributor.title.clone(),
        script_files: snapshot.script.len(),
        localisation_files: snapshot.localisation.len(),
        total_bytes,
        tree_digest: stream.finish(),
        files: if committed { entries } else { BTreeMap::new() },
    })
}
