//! `verify` — the drift gate.
//!
//! Same contract as `tools/oracle/verify.py` and the parser conformance harness's drift gate:
//! recompute every recorded corpus tree digest, re-hash every recorded artifact, compare the
//! recorded decoder, encoder, toolchain, and Stellaris versions against this machine, print `ok`
//! or `DRIFT` per record, and exit non-zero on any drift.
//!
//! Drift is not always wrong. Deliberately changing a fixture means the affected runs must be
//! re-captured, and this names exactly which ones.
//!
//! ```text
//! cargo run --release --manifest-path tools/dds-spike/Cargo.toml --bin verify
//! ```

use dds_spike::corpus::{self, Corpus};
use dds_spike::digest::sha256;
use dds_spike::record::{self, Environment};

fn main() -> std::io::Result<()> {
    let root = record::records_root();
    if !root.is_dir() {
        eprintln!("no records at {}", root.display());
        std::process::exit(1);
    }

    let current = record::environment();
    let mut runs: Vec<_> = std::fs::read_dir(&root)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    runs.sort();

    let mut drifted = 0usize;
    for run in runs {
        let name = run
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let manifest_path = run.join("manifest.json");
        let manifest = match record::read_manifest(&manifest_path) {
            Ok(manifest) => manifest,
            Err(error) => {
                println!("DRIFT {name}: manifest unreadable: {error}");
                drifted += 1;
                continue;
            }
        };

        let mut reasons = Vec::new();

        for (artifact, recorded) in &manifest.artifacts {
            match std::fs::read(run.join(artifact)) {
                Ok(bytes) if sha256(&bytes) == *recorded => {}
                Ok(_) => reasons.push(format!("artifact {artifact} changed")),
                Err(error) => reasons.push(format!("artifact {artifact} unreadable: {error}")),
            }
        }

        for recorded in &manifest.corpora {
            let corpus_entry = Corpus {
                id: recorded.id.clone(),
                title: recorded.title.clone(),
                root: root_for(&recorded.id),
            };
            let files = corpus::enumerate(&corpus_entry.root)?;
            match corpus::identify(&corpus_entry, &files) {
                Ok(current) if current.tree_digest == recorded.tree_digest => {}
                Ok(current) => reasons.push(format!(
                    "corpus {} changed: {} files now, {} recorded",
                    recorded.id, current.file_count, recorded.file_count
                )),
                Err(error) => reasons.push(format!("corpus {} unreadable: {error}", recorded.id)),
            }
        }

        reasons.extend(environment_drift(&manifest.environment, &current));

        if reasons.is_empty() {
            println!("ok    {name}");
        } else {
            drifted += 1;
            println!("DRIFT {name}");
            for reason in reasons {
                println!("        {reason}");
            }
        }
    }

    if drifted > 0 {
        eprintln!("\n{drifted} record(s) drifted; re-capture them or restore their inputs");
        std::process::exit(1);
    }
    println!("\nevery record matches this machine");
    Ok(())
}

fn root_for(id: &str) -> std::path::PathBuf {
    match id {
        "vanilla" => corpus::install_root(),
        "workshop" => corpus::workshop_root(),
        _ => corpus::fixtures_root(),
    }
}

/// Versions that decide a measurement, compared field by field.
///
/// `bcdec_rs` is compared alongside `image_dds` because it is what actually decodes the compressed
/// classes: a bump there changes output pixels while `image_dds`'s own version stays still.
fn environment_drift(recorded: &Environment, current: &Environment) -> Vec<String> {
    let mut reasons = Vec::new();
    let mut check = |label: &str, was: &str, now: &str| {
        if was != now {
            reasons.push(format!("{label}: recorded {was}, current {now}"));
        }
    };
    check("image_dds", &recorded.image_dds, &current.image_dds);
    check("bcdec_rs", &recorded.bcdec_rs, &current.bcdec_rs);
    check(
        "texture2ddecoder",
        &recorded.texture2ddecoder,
        &current.texture2ddecoder,
    );
    check("png", &recorded.png, &current.png);
    check("image-webp", &recorded.image_webp, &current.image_webp);
    check("rustc", &recorded.rustc, &current.rustc);
    check("os", &recorded.os, &current.os);
    check("arch", &recorded.arch, &current.arch);
    for key in ["version", "rawVersion", "modsCompatibilityVersion"] {
        let was = recorded.stellaris.get(key).map(String::as_str).unwrap_or("-");
        let now = current.stellaris.get(key).map(String::as_str).unwrap_or("-");
        check(&format!("stellaris {key}"), was, now);
    }
    reasons
}
