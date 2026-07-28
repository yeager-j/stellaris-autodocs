//! Reading the committed game-oracle records, and the drift comparison that makes an
//! expectation blocking rather than decorative.
//!
//! The records under `docs/spikes/oracle-records/` are the authority for what Stellaris
//! actually did. They are captured by `tools/oracle/capture.py` against a licensed local
//! installation and are not reproducible in CI — but they are *committed*, so a test can
//! hold the resolver to what they say and, just as importantly, notice when what they say
//! changes.
//!
//! Two comparisons, for two different failures:
//!
//! 1. **The build.** Every record names the Stellaris build it was captured against. If that
//!    stops matching the profile's pinned build, some record was re-captured under a new
//!    game version and every resolved cell behind it is unverified until somebody looks. The
//!    design's rule — "a changed result blocks the version update until the Resolution
//!    Profile and golden expectations are intentionally revised" — is this comparison.
//! 2. **The artifacts.** Each manifest declares the digest of the files beside it. A record
//!    edited by hand after capture would otherwise keep reporting a run that never happened,
//!    which is exactly how a pinned expectation quietly becomes a pinned fiction.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::super::profile::StellarisBuild;
use crate::source::fingerprint::ContentHash;

/// Only the fields an expectation depends on. A record holds much more — fixture hashes,
/// activation scope, canaries, the normalized error log — and reading all of it here would
/// couple this gate to parts of the format nothing checks.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct Manifest {
    pub(super) run: String,
    pub(super) stellaris: BTreeMap<String, String>,
    pub(super) artifacts: BTreeMap<String, String>,
}

fn records_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate directory has a parent")
        .join("docs")
        .join("spikes")
        .join("oracle-records")
}

/// The record for `run`.
///
/// Panics when it is missing, rather than skipping. An expectation whose evidence cannot be
/// found has not been checked, and a gate that reports success when it verified nothing is
/// worse than no gate (the standard `parser::conformance` sets for its own corpora).
pub(super) fn read(run: &str) -> Manifest {
    let path = records_root().join(run).join("manifest.json");
    let bytes = fs::read(&path)
        .unwrap_or_else(|error| panic!("oracle record {} is unreadable: {error}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("oracle record {} is undecodable: {error}", path.display()))
}

/// Every way `manifest` disagrees with the profile's pin, or with what is on disk beside it.
///
/// Named differences rather than a bare bool: "the oracle drifted" without saying which
/// field sends the next person back to a diff to find out.
pub(super) fn drift(manifest: &Manifest, pinned: StellarisBuild) -> Vec<String> {
    let mut reasons = Vec::new();
    let run = &manifest.run;

    let absent = "absent".to_owned();
    for (key, expected) in [
        ("version", pinned.version),
        ("rawVersion", pinned.raw_version),
        (
            "modsCompatibilityVersion",
            pinned.mods_compatibility_version,
        ),
    ] {
        let recorded = manifest.stellaris.get(key).unwrap_or(&absent);
        if recorded != expected {
            reasons.push(format!(
                "{run}: stellaris.{key} recorded {recorded}, profile pins {expected}"
            ));
        }
    }

    let directory = records_root().join(run);
    for (artifact, digest) in &manifest.artifacts {
        match fs::read(directory.join(artifact)) {
            Ok(bytes) if ContentHash::of(&bytes).to_hex() == *digest => {}
            Ok(_) => reasons.push(format!("{run}: artifact {artifact} edited after capture")),
            Err(error) => reasons.push(format!("{run}: artifact {artifact} unreadable ({error})")),
        }
    }

    reasons
}

/// What a drifted run must be told, since the instruction is the whole value of the gate.
pub(super) const INSTRUCTION: &str = "\
A record was re-captured, or the profile's pinned build moved. This is not automatically \
wrong — a Stellaris update belongs here — but it means every resolved cell behind these \
records is unverified until somebody re-reads them. Re-run the affected oracle runs, revise \
the expectations against what they now say, and bump both SUPPORTED_STELLARIS_BUILD and \
RESOLUTION_PROFILE_VERSION together. Re-pinning the build alone would make the expectations \
claim evidence that no longer exists.";

#[cfg(test)]
mod tests {
    use super::*;

    fn pinned() -> StellarisBuild {
        super::super::super::profile::SUPPORTED_STELLARIS_BUILD
    }

    #[test]
    fn a_recaptured_build_is_detected() {
        // The negative control for the blocking rule, run through the same comparison the
        // gate uses. Doctored in memory rather than by editing a committed record, so it
        // runs in ordinary CI and cannot corrupt the evidence it is about.
        let mut manifest = read("r10-loadorder");
        manifest
            .stellaris
            .insert("rawVersion".to_owned(), "v4.5.0".to_owned());
        assert_eq!(
            drift(&manifest, pinned()),
            ["r10-loadorder: stellaris.rawVersion recorded v4.5.0, profile pins v4.4.6"]
        );
    }

    #[test]
    fn an_absent_build_field_is_detected_rather_than_read_as_agreement() {
        let mut manifest = read("r10-loadorder");
        manifest.stellaris.remove("version");
        let reasons = drift(&manifest, pinned());
        assert_eq!(
            reasons,
            [
                "r10-loadorder: stellaris.version recorded absent, profile pins Pegasus v4.4.6 (fdde)"
            ]
        );
    }

    #[test]
    fn an_edited_artifact_is_detected() {
        // The other half: the build can match while the evidence beside it has been changed.
        let mut manifest = read("r10-loadorder");
        manifest.artifacts.insert(
            "oracle-facts.txt".to_owned(),
            ContentHash::of(b"not what was captured").to_hex(),
        );
        assert_eq!(
            drift(&manifest, pinned()),
            ["r10-loadorder: artifact oracle-facts.txt edited after capture"]
        );
    }

    #[test]
    fn a_missing_artifact_is_detected() {
        let mut manifest = read("r10-loadorder");
        manifest
            .artifacts
            .insert("never-captured.txt".to_owned(), "0".repeat(64));
        let reasons = drift(&manifest, pinned());
        assert_eq!(reasons.len(), 1, "{reasons:?}");
        assert!(
            reasons[0].starts_with("r10-loadorder: artifact never-captured.txt unreadable"),
            "{}",
            reasons[0]
        );
    }
}
