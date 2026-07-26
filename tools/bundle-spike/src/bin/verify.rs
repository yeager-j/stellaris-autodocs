//! `verify` — does every captured record still describe this machine and these inputs?
//!
//! Same contract as `tools/oracle/verify.py`, `tools/parser-spike/src/bin/verify.rs`, and
//! `tools/dds-spike/src/bin/verify.rs`: recompute every corpus tree digest, re-hash every
//! compared artifact, compare the recorded versions against the current environment, print
//! `ok` or `DRIFT` per record, and exit non-zero on any drift.
//!
//! Two things here are deliberate departures.
//!
//! **Every corpus a run consumes is checked, including committed fixtures.** `d4-failures`
//! reported `ok` while running against a deliberately altered fixture, because its manifest
//! recorded the game corpora but not the fixture corpus it also read. That is a defect in a
//! gate, and it is invisible until someone tries to demonstrate the gate red. This one
//! rebuilds the identity of every corpus the manifest names, and a manifest that names no
//! fixture corpus for a run that reads one is itself reported as drift.
//!
//! **Timing artifacts are not byte-compared, and their exclusion is visible.** They are
//! listed in `uncompared_artifacts` with the reason, so a reader sees that the gate chose not
//! to compare them rather than discovering later that it silently never looked. Their
//! *existence* is still required: a missing timings file is drift.

use bundle_spike::corpus::{self, CorpusIdentity};
use bundle_spike::digest::sha256;
use bundle_spike::record::{self, Environment, Manifest};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let root = record::records_root();
    let Ok(entries) = std::fs::read_dir(&root) else {
        eprintln!("no records at {}", root.display());
        return ExitCode::FAILURE;
    };

    let mut directories: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();

    if directories.is_empty() {
        eprintln!("no records at {}", root.display());
        return ExitCode::FAILURE;
    }

    let current = record::environment();
    let mut identities = IdentityCache::default();
    let mut drifted = false;

    for directory in &directories {
        let name = directory
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<unnamed>");
        let faults = check(directory, &current, &mut identities);
        if faults.is_empty() {
            println!("ok    {name}");
        } else {
            drifted = true;
            println!("DRIFT {name}");
            for fault in faults {
                println!("        {fault}");
            }
        }
    }

    if drifted {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn check(directory: &Path, current: &Environment, identities: &mut IdentityCache) -> Vec<String> {
    let manifest_path = directory.join("manifest.json");
    let manifest = match record::read_manifest(&manifest_path) {
        Ok(manifest) => manifest,
        Err(error) => return vec![format!("manifest unreadable: {error}")],
    };

    let mut faults = Vec::new();
    faults.extend(environment_faults(&manifest.environment, current));
    faults.extend(corpus_faults(&manifest, identities));
    faults.extend(artifact_faults(directory, &manifest));
    faults
}

fn environment_faults(recorded: &Environment, current: &Environment) -> Vec<String> {
    let mut faults = Vec::new();
    let mut compare = |field: &str, was: &str, now: &str| {
        if was != now {
            faults.push(format!("{field}: recorded {was:?}, now {now:?}"));
        }
    };

    compare("jomini", &recorded.jomini, &current.jomini);
    compare("image_dds", &recorded.image_dds, &current.image_dds);
    compare("bcdec_rs", &recorded.bcdec_rs, &current.bcdec_rs);
    compare("png", &recorded.png, &current.png);
    compare(
        "parser_spike_source",
        &recorded.parser_spike_source,
        &current.parser_spike_source,
    );
    compare(
        "dds_spike_source",
        &recorded.dds_spike_source,
        &current.dds_spike_source,
    );
    compare(
        "bundle_spike_source",
        &recorded.bundle_spike_source,
        &current.bundle_spike_source,
    );
    compare("rustc", &recorded.rustc, &current.rustc);
    compare("os", &recorded.os, &current.os);
    compare("arch", &recorded.arch, &current.arch);

    for (key, was) in &recorded.stellaris {
        match current.stellaris.get(key) {
            Some(now) if now == was => {}
            Some(now) => faults.push(format!("stellaris.{key}: recorded {was:?}, now {now:?}")),
            None => faults.push(format!("stellaris.{key}: recorded {was:?}, now absent")),
        }
    }

    faults
}

fn corpus_faults(manifest: &Manifest, identities: &mut IdentityCache) -> Vec<String> {
    let mut faults = Vec::new();
    for recorded in &manifest.corpora {
        match identities.current(&recorded.id) {
            None => faults.push(format!(
                "corpus {:?}: named by the manifest but not a known contributor, so nothing \
                 recomputed it",
                recorded.id
            )),
            Some(Err(error)) => {
                faults.push(format!("corpus {:?}: unreadable: {error}", recorded.id));
            }
            Some(Ok(now)) => faults.extend(compare_identity(recorded, now)),
        }
    }
    faults
}

fn compare_identity(recorded: &CorpusIdentity, now: &CorpusIdentity) -> Vec<String> {
    let mut faults = Vec::new();
    if recorded.tree_digest != now.tree_digest {
        faults.push(format!(
            "corpus {:?}: tree digest recorded {}, now {}",
            recorded.id, recorded.tree_digest, now.tree_digest
        ));
    }
    if recorded.script_files != now.script_files
        || recorded.localisation_files != now.localisation_files
    {
        faults.push(format!(
            "corpus {:?}: file counts recorded {}+{}, now {}+{}",
            recorded.id,
            recorded.script_files,
            recorded.localisation_files,
            now.script_files,
            now.localisation_files
        ));
    }
    if recorded.total_bytes != now.total_bytes {
        faults.push(format!(
            "corpus {:?}: total bytes recorded {}, now {}",
            recorded.id, recorded.total_bytes, now.total_bytes
        ));
    }

    // Per-file digests exist only for committed corpora, and for those they are the reason a
    // single-byte fixture edit is reported as the file it happened to rather than as an
    // opaque tree-digest difference.
    for (path, was) in &recorded.files {
        match now.files.get(path) {
            Some(current) if current == was => {}
            Some(current) => faults.push(format!(
                "corpus {:?}: {path} recorded {was}, now {current}",
                recorded.id
            )),
            None => faults.push(format!("corpus {:?}: {path} recorded, now absent", recorded.id)),
        }
    }
    for path in now.files.keys() {
        if !recorded.files.contains_key(path) {
            faults.push(format!("corpus {:?}: {path} present, not recorded", recorded.id));
        }
    }

    faults
}

fn artifact_faults(directory: &Path, manifest: &Manifest) -> Vec<String> {
    let mut faults = Vec::new();

    for (name, expected) in &manifest.artifacts {
        match std::fs::read(directory.join(name)) {
            Err(error) => faults.push(format!("artifact {name}: unreadable: {error}")),
            Ok(bytes) => {
                let actual = sha256(&bytes);
                if &actual != expected {
                    faults.push(format!(
                        "artifact {name}: recorded {expected}, now {actual}"
                    ));
                }
            }
        }
    }

    // Not byte-compared, but required to exist. An excluded artifact that vanished is still a
    // record describing something that is no longer there.
    for name in manifest.uncompared_artifacts.keys() {
        if !directory.join(name).is_file() {
            faults.push(format!("artifact {name}: declared uncompared, now absent"));
        }
    }

    // A file in the record directory that no manifest entry names is drift in the other
    // direction: something was produced that the record does not account for.
    if let Ok(entries) = std::fs::read_dir(directory) {
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name == "manifest.json"
                || manifest.artifacts.contains_key(&name)
                || manifest.uncompared_artifacts.contains_key(&name)
            {
                continue;
            }
            faults.push(format!("artifact {name}: present, named by no manifest entry"));
        }
    }

    faults
}

/// Recomputed corpus identities, by contributor id.
///
/// Cached because the Vanilla contributor appears in every record and re-enumerating and
/// re-hashing 7,000 files once per record would make the gate slow enough that people stop
/// running it. A gate nobody runs has the same value as no gate.
#[derive(Default)]
struct IdentityCache {
    computed: BTreeMap<String, Result<CorpusIdentity, String>>,
}

impl IdentityCache {
    fn current(&mut self, id: &str) -> Option<&Result<CorpusIdentity, String>> {
        if !self.computed.contains_key(id) {
            let contributor = known_contributor(id)?;
            let identity = corpus::snapshot(&contributor)
                .and_then(|snapshot| corpus::identify(&contributor, &snapshot))
                .map_err(|error| error.to_string());
            self.computed.insert(id.to_owned(), identity);
        }
        self.computed.get(id)
    }
}

/// Every contributor any record may name, including the committed fixtures.
fn known_contributor(id: &str) -> Option<corpus::Contributor> {
    corpus::default_cases()
        .iter()
        .flat_map(|case| case.contributors().into_iter().cloned().collect::<Vec<_>>())
        .find(|contributor| contributor.id == id)
}
