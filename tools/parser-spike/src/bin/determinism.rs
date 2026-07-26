//! `p4-determinism` — the same bytes must produce the same model, wherever they live.
//!
//! Evidence requirement 12: the parsed representation must not depend on the absolute source
//! root, on filesystem enumeration order, or on which contributor a file came from. That is
//! what lets a Revision identifier be a content hash (`docs/technical-design.md:324`) and
//! what makes a rebuild on another machine reproduce a bundle rather than merely resemble
//! one.
//!
//! Three controls, because they can fail independently:
//!
//! 1. **Order.** The corpus is parsed in sorted order and again in reversed order. A digest
//!    that changed would mean state leaking between files.
//! 2. **Parallelism.** Serial against a work-stealing pool. A digest that changed would mean
//!    a data race or an order-dependent accumulator.
//! 3. **Absolute root.** A corpus is copied byte for byte to a temporary directory at a
//!    different absolute path and re-parsed. This is the only one of the three that can
//!    catch a path leaking into the model, and it is checked by copying rather than by
//!    symlinking, because a symlink shares the inode and would pass even if the code
//!    canonicalized the path.
//!
//! ```text
//! cargo run --release --manifest-path tools/parser-spike/Cargo.toml --bin determinism -- --capture
//! ```

use parser_spike::corpus::{self, Corpus, SourceFile};
use parser_spike::digest::digest;
use parser_spike::{lexer, record};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const PURPOSE: &str = "Establish that the application-owned parsed representation depends \
on nothing but the bytes. Each corpus is digested in sorted order, in reversed order, and \
through a parallel pool, and one corpus is copied byte for byte to a different absolute \
path and digested there. The copy is the control that matters: order and parallelism can \
only catch leaked state, while only a genuinely different root can catch a path reaching \
the model. Copying rather than symlinking, because a symlink shares an inode and would pass \
even if the code canonicalized.";

/// The corpus copied to a second absolute root.
///
/// Acquisition of Technology, because it is the smallest real mod and copying 53 MiB of
/// vanilla would trade minutes of I/O for no extra discrimination — the property under test
/// is per-file and does not become more true at scale.
const COPIED: &str = "aot";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Report {
    corpus: String,
    files: usize,
    /// One digest over every file's digest, in normalized logical-path order.
    corpus_digest: String,
    sorted_matches_reversed: bool,
    serial_matches_parallel: bool,
    copied_to_second_root: Option<CopyCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CopyCheck {
    files_compared: usize,
    mismatches: Vec<String>,
    digest_matches: bool,
}

fn digests(files: &[SourceFile]) -> BTreeMap<String, String> {
    files
        .par_iter()
        .filter_map(|file| {
            let data = std::fs::read(&file.absolute).ok()?;
            Some((file.logical.clone(), digest(&lexer::parse(&data))))
        })
        .collect()
}

fn digests_serial(files: &[SourceFile]) -> BTreeMap<String, String> {
    files
        .iter()
        .filter_map(|file| {
            let data = std::fs::read(&file.absolute).ok()?;
            Some((file.logical.clone(), digest(&lexer::parse(&data))))
        })
        .collect()
}

/// Fold per-file digests into one value, in normalized logical-path order.
fn fold(entries: &BTreeMap<String, String>) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"stellaris-docs/corpus-digest/v1\0");
    for (logical, value) in entries {
        hasher.update(logical.as_bytes());
        hasher.update([0]);
        hasher.update(value.as_bytes());
        hasher.update([b'\n']);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<usize> {
    let mut copied = 0usize;
    for entry in std::fs::read_dir(from)?.flatten() {
        let source = entry.path();
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            copied += copy_tree(&source, &target)?;
        } else {
            std::fs::copy(&source, &target)?;
            copied += 1;
        }
    }
    Ok(copied)
}

fn check_copy(corpus: &Corpus, original: &BTreeMap<String, String>) -> std::io::Result<CopyCheck> {
    let root = std::env::temp_dir().join("parser-spike-determinism").join(&corpus.id);
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    std::fs::create_dir_all(&root)?;
    copy_tree(&corpus.root, &root)?;

    let copied_corpus = Corpus {
        id: corpus.id.clone(),
        title: corpus.title.clone(),
        root: root.clone(),
    };
    let files = corpus::enumerate(&copied_corpus.root)?;
    let copied = digests(&files);

    let mut mismatches = Vec::new();
    for (logical, value) in original {
        match copied.get(logical) {
            Some(other) if other == value => {}
            Some(_) => mismatches.push(format!("{logical}\tdigest differs")),
            None => mismatches.push(format!("{logical}\tmissing from the copy")),
        }
    }
    for logical in copied.keys() {
        if !original.contains_key(logical) {
            mismatches.push(format!("{logical}\tpresent only in the copy"));
        }
    }
    mismatches.sort();

    std::fs::remove_dir_all(&root)?;
    Ok(CopyCheck {
        files_compared: copied.len(),
        digest_matches: fold(&copied) == fold(original),
        mismatches,
    })
}

fn main() -> std::io::Result<()> {
    let capture = std::env::args().any(|argument| argument == "--capture");
    let mut reports = Vec::new();
    let mut identities = Vec::new();
    let mut warnings = Vec::new();
    let mut copy_root: Option<PathBuf> = None;

    for corpus in corpus::default_corpora() {
        if !corpus.root.is_dir() {
            warnings.push(format!("corpus `{}` not found; skipped", corpus.id));
            continue;
        }
        let mut files = corpus::enumerate(&corpus.root)?;
        let sorted = digests(&files);

        files.reverse();
        let reversed = digests(&files);
        files.reverse();

        let serial = digests_serial(&files);

        let copy = if corpus.id == COPIED {
            let check = check_copy(&corpus, &sorted)?;
            copy_root = Some(std::env::temp_dir().join("parser-spike-determinism"));
            Some(check)
        } else {
            None
        };

        let report = Report {
            corpus: corpus.id.clone(),
            files: sorted.len(),
            corpus_digest: fold(&sorted),
            sorted_matches_reversed: fold(&sorted) == fold(&reversed),
            serial_matches_parallel: fold(&sorted) == fold(&serial),
            copied_to_second_root: copy,
        };

        println!(
            "{:<10} files {:>5}  order {}  parallel {}  copiedRoot {}",
            report.corpus,
            report.files,
            if report.sorted_matches_reversed { "ok" } else { "DIFFERS" },
            if report.serial_matches_parallel { "ok" } else { "DIFFERS" },
            match &report.copied_to_second_root {
                None => "-".to_string(),
                Some(check) if check.digest_matches && check.mismatches.is_empty() =>
                    format!("ok ({} files)", check.files_compared),
                Some(check) => format!("DIFFERS ({} mismatches)", check.mismatches.len()),
            }
        );
        println!("           digest {}", report.corpus_digest);

        reports.push(report);
        identities.push(corpus::identify(&corpus, &files)?);
    }

    if let Some(root) = copy_root {
        let _ = std::fs::remove_dir_all(root);
    }

    if !capture {
        eprintln!("(not captured; pass --capture to write the record)");
        return Ok(());
    }

    let mut json = serde_json::to_string_pretty(&reports)?;
    json.push('\n');
    let directory = record::write(
        "p4-determinism",
        PURPOSE,
        identities,
        vec![("determinism.json".to_owned(), json)],
        warnings,
    )?;
    eprintln!("captured {}", directory.display());
    Ok(())
}
