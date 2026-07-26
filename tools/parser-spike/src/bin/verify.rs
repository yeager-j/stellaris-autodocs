//! The drift gate over captured records.
//!
//! Same contract as `tools/oracle/verify.py`: recompute what each record claims about its
//! inputs and environment, print `ok` or `DRIFT` per record, and exit non-zero if anything
//! moved. A record captured against another game build, another Jomini version, or a
//! changed corpus documents that world rather than this one, and nothing else in the
//! repository would notice.
//!
//! ```text
//! cargo run --release --manifest-path tools/parser-spike/Cargo.toml --bin verify
//! ```
//!
//! Prove it can fail before trusting it, as `AGENTS.md` requires of any gate:
//!
//! ```text
//! STELLARIS_WORKSHOP_ROOT=/tmp/somewhere-else cargo run --bin verify
//! ```

use parser_spike::corpus::{self, Corpus};
use parser_spike::record;
use std::collections::BTreeMap;

fn main() -> std::io::Result<()> {
    let root = record::records_root();
    if !root.is_dir() {
        eprintln!("no records at {}", root.display());
        std::process::exit(1);
    }

    let current = record::environment();
    let corpora: BTreeMap<String, Corpus> = corpus::default_corpora()
        .into_iter()
        .map(|corpus| (corpus.id.clone(), corpus))
        .collect();

    // Corpus digests are recomputed once and reused: every record pins the same corpora, and
    // hashing 100 MiB per record would make the gate too slow to run habitually, which is
    // the only way a gate is useful.
    let mut computed: BTreeMap<String, Option<String>> = BTreeMap::new();
    for (id, corpus) in &corpora {
        let digest = if corpus.root.is_dir() {
            let files = corpus::enumerate(&corpus.root)?;
            Some(corpus::identify(corpus, &files)?.tree_digest)
        } else {
            None
        };
        computed.insert(id.clone(), digest);
    }

    let mut directories: Vec<_> = std::fs::read_dir(&root)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();

    let mut drifted = 0usize;
    for directory in &directories {
        let name = directory
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let manifest = match record::read_manifest(&directory.join("manifest.json")) {
            Ok(manifest) => manifest,
            Err(error) => {
                println!("  DRIFT  {name}: unreadable manifest: {error}");
                drifted += 1;
                continue;
            }
        };

        let mut problems = Vec::new();

        if manifest.environment.jomini != current.jomini {
            problems.push(format!(
                "jomini {} captured, {} installed",
                manifest.environment.jomini, current.jomini
            ));
        }
        if manifest.environment.rustc != current.rustc {
            problems.push(format!(
                "rustc {:?} captured, {:?} installed",
                manifest.environment.rustc, current.rustc
            ));
        }
        let captured_build = manifest.environment.stellaris.get("version");
        let current_build = current.stellaris.get("version");
        if captured_build != current_build {
            problems.push(format!(
                "Stellaris {captured_build:?} captured, {current_build:?} installed"
            ));
        }

        for recorded in &manifest.corpora {
            match computed.get(&recorded.id) {
                None => problems.push(format!("corpus `{}` is no longer defined", recorded.id)),
                Some(None) => problems.push(format!("corpus `{}` is not present", recorded.id)),
                Some(Some(digest)) if *digest != recorded.tree_digest => problems.push(format!(
                    "corpus `{}` changed ({} files at capture)",
                    recorded.id, recorded.file_count
                )),
                Some(Some(_)) => {}
            }
        }

        for (artifact, expected) in &manifest.artifacts {
            match std::fs::read(directory.join(artifact)) {
                Err(error) => problems.push(format!("artifact `{artifact}` unreadable: {error}")),
                Ok(bytes) if corpus::sha256(&bytes) != *expected => {
                    problems.push(format!("artifact `{artifact}` was edited after capture"))
                }
                Ok(_) => {}
            }
        }

        if problems.is_empty() {
            println!("  ok     {name}");
        } else {
            drifted += 1;
            println!("  DRIFT  {name}");
            for problem in problems {
                println!("           {problem}");
            }
        }
        for warning in &manifest.warnings {
            println!("           warning at capture: {warning}");
        }
    }

    println!(
        "\n{} records, {} drifted",
        directories.len(),
        drifted
    );
    if drifted > 0 {
        std::process::exit(1);
    }
    Ok(())
}
