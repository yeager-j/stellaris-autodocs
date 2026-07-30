//! Where the licensed local corpora live, and what establishing one means for a
//! corpus-facing test run.
//!
//! Two harnesses read the installed corpora — the parser's whole-corpus conformance run
//! ([`super::parser::conformance`]) and the resolver's embedded-parameter census
//! ([`super::resolver::census`]) — and both have to agree on three things: where Stellaris is,
//! which Workshop mod ACOT is, and what a missing corpus means. Homed here so neither harness
//! becomes a second authority on any of them; in particular so "a missing corpus fails the
//! run, it never skips it" is one rule rather than two copies that can drift apart.
//!
//! Nothing here is a product path. The application finds installations through
//! [`crate::discovery`] and user-confirmed Discovery Locations; these are the developer
//! machine's Steam defaults, overridable by environment variable, and they exist only so a
//! test can be pointed at a corpus.

use std::path::{Path, PathBuf};

use crate::source::snapshot::{Established, LiveSource, SourceKind, establish};

/// ACOT's Workshop identifier. The Workshop addresses mods by number, and the number is what
/// a record can be reproduced from; the title is for humans.
const ACOT_WORKSHOP_ID: &str = "1419304439";

/// One corpus a run reads: what it is, and where its root sits.
pub(super) struct Corpus {
    pub id: &'static str,
    pub title: &'static str,
    pub kind: SourceKind,
    pub root: PathBuf,
}

/// An environment override, or a `~`-expanded default. An empty value counts as unset, so
/// exporting the variable empty does not silently point the run at `/`.
fn env_path(name: &str, default: &str) -> PathBuf {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => expand_home(default),
    }
}

fn expand_home(raw: &str) -> PathBuf {
    match raw.strip_prefix("~/") {
        Some(rest) => match std::env::var("HOME") {
            Ok(home) => PathBuf::from(home).join(rest),
            Err(_) => PathBuf::from(raw),
        },
        None => PathBuf::from(raw),
    }
}

pub(super) fn install_root() -> PathBuf {
    env_path(
        "STELLARIS_INSTALL_ROOT",
        "~/Library/Application Support/Steam/steamapps/common/Stellaris",
    )
}

fn workshop_root() -> PathBuf {
    env_path(
        "STELLARIS_WORKSHOP_ROOT",
        "~/Library/Application Support/Steam/steamapps/workshop/content/281990",
    )
}

/// The content every mod is documented against.
pub(super) fn vanilla() -> Corpus {
    Corpus {
        id: "vanilla",
        title: "Stellaris base game",
        kind: SourceKind::VanillaContent,
        root: install_root(),
    }
}

/// The Target Mod the product's golden cases are built around, and the corpus that produced
/// the parser spike's stray-token finding.
pub(super) fn acot() -> Corpus {
    Corpus {
        id: "acot",
        title: "Ancient Cache of Technologies",
        kind: SourceKind::TargetMod,
        root: workshop_root().join(ACOT_WORKSHOP_ID),
    }
}

/// A corpus established through the production enumeration, with any completeness gap
/// reported rather than swallowed.
///
/// A missing root **fails**; it never skips. A run that quietly reports success when it
/// verified nothing is worse than no run at all — which is why the absent-corpus case is an
/// assertion here and not an early `None` for a caller to interpret.
pub(super) fn establish_corpus(corpus: &Corpus) -> (LiveSource, Vec<String>) {
    assert!(
        corpus.root.is_dir(),
        "corpus {} is not installed at {}. Set STELLARIS_INSTALL_ROOT / \
         STELLARIS_WORKSHOP_ROOT, or do not ask for this run: a missing corpus is a failed \
         run, never a silent pass.",
        corpus.id,
        corpus.root.display()
    );
    let established = establish(corpus.kind, &corpus.root)
        .unwrap_or_else(|error| panic!("corpus {} could not be established: {error}", corpus.id));
    match established {
        Established::Complete(source) => (source, Vec::new()),
        // Publishable, and worth recording: a gap is content the observation could not see,
        // so a run over an incomplete corpus is measuring slightly less than it claims.
        Established::Incomplete(source) => {
            let warning = format!(
                "corpus {} established incomplete: {:?}",
                corpus.id,
                source.snapshot().gaps()
            );
            (source, vec![warning])
        }
    }
}

/// The repository root, for harnesses that read or write committed records.
pub(super) fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate directory has a parent")
        .to_path_buf()
}
