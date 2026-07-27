# Phase 1 — Durable State and Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The durable-state module with its crash-safe replacement and recovery protocol, stable Discovery Location / Mod Installation identity, and headless discovery scanning that derives the Mod Library — everything Phase 3's publication pointer and acceptance harness stand on.

**Architecture:** Three ticket-sized clusters. **A (identity)**: `discovery::identity` owns `DiscoveryLocationId` (random at creation — a location's path is editable configuration, not identity) and `ModInstallationId` (canonical digest of location id + normalized relative path). **B (state)**: `state` owns the single versioned JSON document, the atomic-replacement protocol behind an injectable I/O seam, quarantine recovery, the newer-schema block, and typed mutations including the narrow publication-reference CAS that `revisions` consumes in Phase 3. **C (discovery)**: `discovery` scans configured locations into derived Mod Library contents (installations, visible collisions, rejected entries, unavailable locations) plus first-run proposals and Stellaris install metadata. Dependency direction: `state` imports identity types from `discovery::identity`; `discovery` never imports `state` (the application layer will map stored locations into scan inputs in Phase 3).

**Tech Stack:** Phase 0 primitives (`canonical::{encode,path}`, `error`, `testsupport`), `serde`/`serde_json`, `uuid`, `sha2`, `tempfile`, `proptest`.

## Global Constraints

- Persistence principle: **derive what you can; store what you must** (`docs/technical-design.md`, "Persistence principle"). Discovery reports what the current scan observes; state never becomes a mutable duplicate of the Mod Library.
- The state document's normative commit point is atomic replacement of the state path. Ambiguous outcomes are resolved by reopening and validating the authoritative path, never inferred from the error alone.
- A newer-schema state file is never overwritten and never receives any write. Malformed state is quarantined (timestamp + content hash in the name), never overwritten in place.
- Identity: installation identity = location identifier + normalized relative path (NFC, case-preserving). Absolute paths, titles, declared versions, and fingerprints never enter identifiers.
- Collisions (two raw entries → one logical path) are visible results, never an arbitrary winner. Invalid Unicode is rejected without lossy conversion (lossy text may appear only in human-facing report labels, never identity).
- Descriptor metadata is advisory; discovery stays lightweight and never parses complete content (`docs/technical-design.md`, "Source module").
- All Phase 0 gates stay green: `tools/ci/check.sh` exits 0 at every commit. TDD: watch each test fail first.

**Design decisions this plan makes (record in each ticket's PR, decision log at phase end):**

1. `state` stores each publication reference as `{ location, revision }` keyed by installation id. The location component is *not* recoverable from the digest-valued installation id, and removing a Discovery Location must cascade its references in one mutation, so storing it is necessary, not redundant.
2. The quarantine notice (`unresolved_quarantine`) is persisted in the state document, not held only in process memory — otherwise a restart would silently forget that publication-reference recovery is unresolved and re-enable cleanup the design disables.
3. Only immediate child directories of a Discovery Location are Mod Installations. A root-level `.mod` descriptor whose `path` points outside the location is advisory metadata, not an installation — the identity model (location + relative path) cannot address content outside the location.
4. Collision classification is a pure function over raw names. macOS APFS is case- and normalization-insensitive, so colliding fixtures cannot be created on the development filesystem; the pure seam is what makes the rule testable at all.

---

## Ticket A — Identity (plan Tasks 1–2)

### Task 1: Identifier types and derivation

**Files:**
- Create: `src-tauri/src/discovery/identity.rs`
- Modify: `src-tauri/src/discovery/mod.rs` (add `pub mod identity;` below the doc comment)

**Interfaces:**
- Consumes: `canonical::encode::CanonicalDigest`, `canonical::path::LogicalPath`, `uuid`.
- Produces: `DiscoveryLocationId` (`generate()`, `parse(&str)`, `Display` as 32 hex chars), `ModInstallationId` (`derive(DiscoveryLocationId, &LogicalPath)`, `parse(&str)`, `Display` as 64 hex chars), `IdParseError`. Task 3's state model and Task 9's scanner consume these.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/discovery/identity.rs` with only this test module, and register the submodule:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::path::LogicalPath;

    fn path(raw: &str) -> LogicalPath {
        LogicalPath::parse(raw).unwrap()
    }

    #[test]
    fn location_ids_are_32_hex_and_unique_and_round_trip() {
        let id = DiscoveryLocationId::generate();
        let rendered = id.to_string();
        assert_eq!(rendered.len(), 32);
        assert!(rendered.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(id, DiscoveryLocationId::generate());
        assert_eq!(DiscoveryLocationId::parse(&rendered), Ok(id));
        assert_eq!(DiscoveryLocationId::parse("zz"), Err(IdParseError));
    }

    #[test]
    fn installation_id_is_deterministic_for_location_and_path() {
        let location = DiscoveryLocationId::generate();
        let first = ModInstallationId::derive(location, &path("ugc_123"));
        let second = ModInstallationId::derive(location, &path("ugc_123"));
        assert_eq!(first, second);
        let rendered = first.to_string();
        assert_eq!(rendered.len(), 64);
        assert_eq!(ModInstallationId::parse(&rendered), Ok(first));
    }

    #[test]
    fn different_location_or_path_changes_the_identity() {
        let here = DiscoveryLocationId::generate();
        let there = DiscoveryLocationId::generate();
        let base = ModInstallationId::derive(here, &path("acot"));
        assert_ne!(base, ModInstallationId::derive(there, &path("acot")));
        assert_ne!(base, ModInstallationId::derive(here, &path("acot2")));
    }

    #[test]
    fn case_only_path_variants_are_distinct_identities() {
        let location = DiscoveryLocationId::generate();
        assert_ne!(
            ModInstallationId::derive(location, &path("Giga")),
            ModInstallationId::derive(location, &path("giga"))
        );
    }

    #[test]
    fn nfc_equivalent_paths_share_one_identity() {
        // The same rename that changes bytes but not the logical path must not change
        // identity — this is what lets preferences and references survive re-scans.
        let location = DiscoveryLocationId::generate();
        assert_eq!(
            ModInstallationId::derive(location, &path("te\u{301}ch")),
            ModInstallationId::derive(location, &path("t\u{e9}ch"))
        );
    }
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support discovery::identity::`
Expected: compile error — `DiscoveryLocationId` not found.

- [ ] **Step 3: Implement**

Prepend above the test module:

```rust
//! Stable identifiers for Discovery Locations and Mod Installations
//! (docs/technical-design.md, "Installation identity").
//!
//! A Discovery Location identifier is random at creation: the location's absolute path
//! is editable configuration rather than identity, so a rebind preserves it. A Mod
//! Installation identifier is the canonical digest of the location identifier plus the
//! normalized relative mod path: re-scanning is stable, editing a location's path
//! preserves derived identities, and moving a mod within a location creates a new one.
//! Absolute paths, titles, declared versions, and content fingerprints never enter.

use crate::canonical::encode::CanonicalDigest;
use crate::canonical::path::LogicalPath;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DiscoveryLocationId([u8; 16]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdParseError;

impl DiscoveryLocationId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().into_bytes())
    }

    pub fn parse(text: &str) -> Result<Self, IdParseError> {
        decode_hex::<16>(text).map(Self).ok_or(IdParseError)
    }
}

impl fmt::Display for DiscoveryLocationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(f, &self.0)
    }
}

/// Opaque and deterministic within the app's identifier scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModInstallationId([u8; 32]);

impl ModInstallationId {
    pub fn derive(location: DiscoveryLocationId, mod_root: &LogicalPath) -> Self {
        let mut digest = CanonicalDigest::new("stellaris-docs/mod-installation-id/v1");
        digest.text(&location.to_string()).text(mod_root.as_str());
        Self(digest.finish().0)
    }

    pub fn parse(text: &str) -> Result<Self, IdParseError> {
        decode_hex::<32>(text).map(Self).ok_or(IdParseError)
    }
}

impl fmt::Display for ModInstallationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(f, &self.0)
    }
}

fn write_hex(f: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    for byte in bytes {
        write!(f, "{byte:02x}")?;
    }
    Ok(())
}

fn decode_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let bytes = text.as_bytes();
    if bytes.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (slot, pair) in out.iter_mut().zip(bytes.chunks_exact(2)) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        *slot = ((hi << 4) | lo) as u8;
    }
    Some(out)
}
```

- [ ] **Step 4: Run and watch it pass**

Run: `cargo test --features test-support discovery::identity::`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/discovery
git commit -m "Phase 1: discovery and installation identity derivation"
```

### Task 2: Serde representations and property suite

**Files:**
- Modify: `src-tauri/src/discovery/identity.rs`

**Interfaces:**
- Produces: `Serialize`/`Deserialize` for both id types as their hex strings (usable as JSON map keys). Task 3's state model relies on this exact representation.

- [ ] **Step 1: Write the failing tests**

Append to the test module in `identity.rs`:

```rust
    use proptest::prelude::*;

    #[test]
    fn ids_serialize_as_hex_strings_and_round_trip_as_map_keys() {
        let location = DiscoveryLocationId::generate();
        let installation = ModInstallationId::derive(location, &path("ugc_1"));
        let json = serde_json::to_string(&location).unwrap();
        assert_eq!(json, format!("\"{location}\""));

        let map = std::collections::BTreeMap::from([(installation, 1u32)]);
        let encoded = serde_json::to_string(&map).unwrap();
        let decoded: std::collections::BTreeMap<ModInstallationId, u32> =
            serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, map);

        assert!(serde_json::from_str::<DiscoveryLocationId>("\"not-hex\"").is_err());
    }

    const COMPONENT_RE: &str = "[a-z0-9_\u{e9}]{1,10}(/[a-z0-9_\u{e9}]{1,10}){0,2}";

    proptest! {
        #[test]
        fn derivation_is_injective_over_generated_inputs(
            a in COMPONENT_RE,
            b in COMPONENT_RE,
        ) {
            // Same location: distinct logical paths must yield distinct identities.
            // Digest framing (Phase 0) is what rules out concatenation collisions.
            let location = DiscoveryLocationId::parse(
                "00000000000000000000000000000001",
            ).unwrap();
            let left = ModInstallationId::derive(location, &path(&a));
            let right = ModInstallationId::derive(location, &path(&b));
            prop_assert_eq!(left == right, path(&a) == path(&b));
        }

        #[test]
        fn display_parse_round_trips(seed in any::<[u8; 16]>()) {
            let rendered = DiscoveryLocationId::parse(
                &seed.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            ).unwrap();
            prop_assert_eq!(DiscoveryLocationId::parse(&rendered.to_string()), Ok(rendered));
        }
    }
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support discovery::identity::`
Expected: compile error — `Serialize` not implemented.

- [ ] **Step 3: Implement**

Add to `identity.rs` (below the `decode_hex` helper):

```rust
macro_rules! hex_string_serde {
    ($ty:ident, $expected:literal) => {
        impl serde::Serialize for $ty {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }
        impl<'de> serde::Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let text = <std::borrow::Cow<'de, str>>::deserialize(d)?;
                $ty::parse(&text).map_err(|_| serde::de::Error::custom($expected))
            }
        }
    };
}

hex_string_serde!(DiscoveryLocationId, "expected 32 lowercase hex characters");
hex_string_serde!(ModInstallationId, "expected 64 lowercase hex characters");
```

- [ ] **Step 4: Run and watch it pass**

Run: `cargo test --features test-support discovery::identity::`
Expected: all unit tests + 2 property tests pass.

- [ ] **Step 5: Run the full gate and commit**

Run: `tools/ci/check.sh` — expected exit 0.

```bash
git add src-tauri/src/discovery
git commit -m "Phase 1: identity serde representations and property suite"
```

---

## Ticket B — State module (plan Tasks 3–7)

### Task 3: State model and schema round-trips

**Files:**
- Create: `src-tauri/src/state/model.rs`
- Modify: `src-tauri/src/state/mod.rs` (add `pub mod model;` below the doc comment)

**Interfaces:**
- Consumes: `discovery::identity::{DiscoveryLocationId, ModInstallationId}`.
- Produces: `CURRENT_SCHEMA: u32 = 1`; `RevisionId(pub String)`; `DiscoveryLocation { id, path }`; `PublicationReference { location, revision }`; `AppState { schema, discovery_locations, publication_references, unresolved_quarantine }` with `AppState::first_launch()`. Tasks 4–7 and Phase 3's `revisions` consume these.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/state/model.rs` with only this test module and register it:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_with_every_optional_field_absent() {
        // Manifest-style validation hashes bytes without parsing them, so an
        // undecodable-but-intact document is the failure mode to guard against
        // (docs/technical-design.md, "Materialized JSON read model").
        let minimal: AppState = serde_json::from_str(r#"{"schema":1}"#).unwrap();
        assert_eq!(minimal, AppState::first_launch());
    }

    #[test]
    fn round_trips_a_populated_state() {
        use crate::canonical::path::LogicalPath;
        use crate::discovery::identity::{DiscoveryLocationId, ModInstallationId};

        let location = DiscoveryLocationId::generate();
        let installation =
            ModInstallationId::derive(location, &LogicalPath::parse("ugc_1").unwrap());
        let state = AppState {
            schema: CURRENT_SCHEMA,
            discovery_locations: vec![DiscoveryLocation {
                id: location,
                path: std::path::PathBuf::from("/tmp/workshop"),
            }],
            publication_references: std::collections::BTreeMap::from([(
                installation,
                PublicationReference {
                    location,
                    revision: RevisionId("abc123".to_owned()),
                },
            )]),
            unresolved_quarantine: Some("state.json.quarantine-1-deadbeef".to_owned()),
        };
        let encoded = serde_json::to_vec(&state).unwrap();
        let decoded: AppState = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, state);
    }
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support state::model::`
Expected: compile error — `AppState` not found.

- [ ] **Step 3: Implement**

Prepend above the test module:

```rust
//! The durable mutable state document (docs/technical-design.md, "Durable mutable
//! state"): user intent, publication references, and the schema version required to
//! interpret them. Everything else about the Mod Library is derived at scan time.

use crate::discovery::identity::{DiscoveryLocationId, ModInstallationId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const CURRENT_SCHEMA: u32 = 1;

/// Opaque published-revision reference. Phase 3 derives it from the canonical revision
/// manifest digest; state preserves it as an exact round-trip token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RevisionId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryLocation {
    pub id: DiscoveryLocationId,
    /// Editable configuration, not identity: a rebind changes this and nothing else.
    pub path: PathBuf,
}

/// The atomically published revision for one installation. `location` is stored because
/// it is not recoverable from the digest-valued installation id, and removing a
/// Discovery Location must cascade its references in one mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationReference {
    pub location: DiscoveryLocationId,
    pub revision: RevisionId,
}

/// Every collection and option carries a read-side default so a document with the
/// field absent still decodes — the revision-bundle spike's lesson.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppState {
    pub schema: u32,
    #[serde(default)]
    pub discovery_locations: Vec<DiscoveryLocation>,
    #[serde(default)]
    pub publication_references: BTreeMap<ModInstallationId, PublicationReference>,
    /// Set when unreadable state was quarantined; cleared only by explicit user
    /// confirmation. Persisted so a restart cannot silently forget that
    /// publication-reference recovery is unresolved. While present, orphan-revision
    /// and Asset Store cleanup stay disabled (docs/technical-design.md, "State
    /// evolution and recovery").
    #[serde(default)]
    pub unresolved_quarantine: Option<String>,
}

impl AppState {
    pub fn first_launch() -> Self {
        Self {
            schema: CURRENT_SCHEMA,
            discovery_locations: Vec::new(),
            publication_references: BTreeMap::new(),
            unresolved_quarantine: None,
        }
    }
}
```

- [ ] **Step 4: Run, pass, commit**

Run: `cargo test --features test-support state::model::` — expected: 2 passed.

```bash
git add src-tauri/src/state
git commit -m "Phase 1: durable state model and schema round-trips"
```

### Task 4: Replacement protocol with injectable failures

**Files:**
- Create: `src-tauri/src/state/replace.rs`
- Modify: `src-tauri/src/state/mod.rs` (add `pub mod replace;`)

**Interfaces:**
- Produces: `ReplacementIo` trait (`write_temp`, `rename`, `sync_dir`), `RealIo`, `ReplaceOutcome { Committed, CommittedDurabilityUncertain, PriorRetained { detail }, RecoveryRequired { detail } }`, `replace_state(io, state_path, next_bytes, prior_bytes) -> ReplaceOutcome`. Task 5's store commits every mutation through this; Phase 3's publication pointer inherits its semantics.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/state/replace.rs` with only this test module and register it:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    const PRIOR: &[u8] = br#"{"schema":1,"marker":"prior"}"#;
    const NEXT: &[u8] = br#"{"schema":1,"marker":"next"}"#;

    fn seeded_dir() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        fs::write(&state_path, PRIOR).unwrap();
        (dir, state_path)
    }

    /// Fails exactly one protocol step; every other step is real.
    struct FailAt {
        step: &'static str,
        inner: RealIo,
    }

    impl ReplacementIo for FailAt {
        fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
            if self.step == "write_temp" {
                return Err(io::Error::other("injected write_temp failure"));
            }
            self.inner.write_temp(dir, bytes)
        }
        fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
            if self.step == "rename" {
                return Err(io::Error::other("injected rename failure"));
            }
            self.inner.rename(from, to)
        }
        fn sync_dir(&mut self, dir: &Path) -> io::Result<()> {
            if self.step == "sync_dir" {
                return Err(io::Error::other("injected sync_dir failure"));
            }
            self.inner.sync_dir(dir)
        }
    }

    #[test]
    fn success_commits_and_replaces_the_file() {
        let (_dir, state_path) = seeded_dir();
        let outcome = replace_state(&mut RealIo, &state_path, NEXT, Some(PRIOR));
        assert_eq!(outcome, ReplaceOutcome::Committed);
        assert_eq!(fs::read(&state_path).unwrap(), NEXT);
    }

    #[test]
    fn failure_before_the_commit_point_retains_the_prior_file() {
        let (_dir, state_path) = seeded_dir();
        let mut io_seam = FailAt { step: "write_temp", inner: RealIo };
        let outcome = replace_state(&mut io_seam, &state_path, NEXT, Some(PRIOR));
        assert!(matches!(outcome, ReplaceOutcome::PriorRetained { .. }));
        assert_eq!(fs::read(&state_path).unwrap(), PRIOR);
    }

    #[test]
    fn rename_error_with_prior_still_on_disk_retains_prior() {
        let (_dir, state_path) = seeded_dir();
        let mut io_seam = FailAt { step: "rename", inner: RealIo };
        let outcome = replace_state(&mut io_seam, &state_path, NEXT, Some(PRIOR));
        assert!(matches!(outcome, ReplaceOutcome::PriorRetained { .. }));
        assert_eq!(fs::read(&state_path).unwrap(), PRIOR);
    }

    #[test]
    fn rename_that_succeeded_but_reported_failure_is_committed_uncertain() {
        // The authoritative path, not the error, decides
        // (docs/technical-design.md, "Mutable state storage").
        struct LyingRename;
        impl ReplacementIo for LyingRename {
            fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
                RealIo.write_temp(dir, bytes)
            }
            fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
                fs::rename(from, to)?;
                Err(io::Error::other("injected post-rename failure report"))
            }
            fn sync_dir(&mut self, _dir: &Path) -> io::Result<()> {
                Ok(())
            }
        }
        let (_dir, state_path) = seeded_dir();
        let outcome = replace_state(&mut LyingRename, &state_path, NEXT, Some(PRIOR));
        assert_eq!(outcome, ReplaceOutcome::CommittedDurabilityUncertain);
        assert_eq!(fs::read(&state_path).unwrap(), NEXT);
    }

    #[test]
    fn sync_dir_failure_after_rename_is_committed_uncertain() {
        let (_dir, state_path) = seeded_dir();
        let mut io_seam = FailAt { step: "sync_dir", inner: RealIo };
        let outcome = replace_state(&mut io_seam, &state_path, NEXT, Some(PRIOR));
        assert_eq!(outcome, ReplaceOutcome::CommittedDurabilityUncertain);
        assert_eq!(fs::read(&state_path).unwrap(), NEXT);
    }

    #[test]
    fn unrecognizable_authoritative_content_requires_recovery() {
        // Rename fails AND the authoritative file no longer matches prior or next:
        // callers must stop mutating rather than guess.
        struct CorruptingRename;
        impl ReplacementIo for CorruptingRename {
            fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
                RealIo.write_temp(dir, bytes)
            }
            fn rename(&mut self, _from: &Path, to: &Path) -> io::Result<()> {
                fs::write(to, b"neither prior nor next").unwrap();
                Err(io::Error::other("injected rename failure"))
            }
            fn sync_dir(&mut self, _dir: &Path) -> io::Result<()> {
                Ok(())
            }
        }
        let (_dir, state_path) = seeded_dir();
        let outcome = replace_state(&mut CorruptingRename, &state_path, NEXT, Some(PRIOR));
        assert!(matches!(outcome, ReplaceOutcome::RecoveryRequired { .. }));
    }

    #[test]
    fn first_write_with_no_prior_file_commits() {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        let outcome = replace_state(&mut RealIo, &state_path, NEXT, None);
        assert_eq!(outcome, ReplaceOutcome::Committed);
        assert_eq!(fs::read(&state_path).unwrap(), NEXT);
    }
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support state::replace::`
Expected: compile error — `ReplacementIo` not found.

- [ ] **Step 3: Implement**

Prepend above the test module:

```rust
//! Crash-safe replacement of the state document (docs/technical-design.md, "Mutable
//! state storage").
//!
//! The normative commit point is the atomic rename onto the state path. Failure before
//! it leaves the prior file authoritative. A failure report at or after it is ambiguous
//! evidence: the outcome is decided by reopening and comparing the authoritative path
//! against the known prior and next bytes, never inferred from the error alone.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The externally observable I/O steps of one replacement, in protocol order.
/// A seam so tests can fail each step; production is [`RealIo`].
pub trait ReplacementIo {
    /// Create, write, and durably flush a uniquely named temporary file in `dir`.
    fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf>;
    /// Atomically rename `from` onto `to`. The commit point.
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()>;
    /// Durably flush the directory entry change where the platform provides it.
    fn sync_dir(&mut self, dir: &Path) -> io::Result<()>;
}

pub struct RealIo;

impl ReplacementIo for RealIo {
    fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
        let path = dir.join(format!(".state-{}.tmp", uuid::Uuid::new_v4()));
        let mut file = fs::File::create(&path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(path)
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn sync_dir(&mut self, dir: &Path) -> io::Result<()> {
        fs::File::open(dir)?.sync_all()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReplaceOutcome {
    Committed,
    /// The new state is visible on the authoritative path but its durability could not
    /// be confirmed.
    CommittedDurabilityUncertain,
    /// Failure before the commit point; the prior file remains authoritative.
    PriorRetained { detail: String },
    /// The authoritative path matches neither prior nor next; mutation must stop and
    /// state recovery begins.
    RecoveryRequired { detail: String },
}

pub fn replace_state(
    io_seam: &mut dyn ReplacementIo,
    state_path: &Path,
    next_bytes: &[u8],
    prior_bytes: Option<&[u8]>,
) -> ReplaceOutcome {
    let Some(dir) = state_path.parent() else {
        return ReplaceOutcome::PriorRetained {
            detail: "state path has no parent directory".to_owned(),
        };
    };
    let temp = match io_seam.write_temp(dir, next_bytes) {
        Ok(temp) => temp,
        Err(error) => {
            return ReplaceOutcome::PriorRetained {
                detail: format!("writing temporary state: {error}"),
            };
        }
    };
    if let Err(error) = io_seam.rename(&temp, state_path) {
        let _ = fs::remove_file(&temp);
        return classify_by_reread(
            state_path,
            next_bytes,
            prior_bytes,
            &format!("rename onto state path failed: {error}"),
        );
    }
    match io_seam.sync_dir(dir) {
        Ok(()) => ReplaceOutcome::Committed,
        Err(error) => classify_by_reread(
            state_path,
            next_bytes,
            prior_bytes,
            &format!("directory sync failed: {error}"),
        ),
    }
}

fn classify_by_reread(
    state_path: &Path,
    next_bytes: &[u8],
    prior_bytes: Option<&[u8]>,
    detail: &str,
) -> ReplaceOutcome {
    match fs::read(state_path) {
        Ok(found) if found == next_bytes => ReplaceOutcome::CommittedDurabilityUncertain,
        Ok(found) if prior_bytes == Some(found.as_slice()) => ReplaceOutcome::PriorRetained {
            detail: detail.to_owned(),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound && prior_bytes.is_none() => {
            ReplaceOutcome::PriorRetained {
                detail: detail.to_owned(),
            }
        }
        _ => ReplaceOutcome::RecoveryRequired {
            detail: detail.to_owned(),
        },
    }
}
```

- [ ] **Step 4: Run, pass, commit**

Run: `cargo test --features test-support state::replace::` — expected: 7 passed.

```bash
git add src-tauri/src/state
git commit -m "Phase 1: crash-safe state replacement protocol"
```

### Task 5: Store open — first launch, quarantine, newer-schema block

**Files:**
- Create: `src-tauri/src/state/store.rs`
- Modify: `src-tauri/src/state/mod.rs` (add `pub mod store;`)

**Interfaces:**
- Consumes: Tasks 3–4; `sha2`, `tempfile` (tests).
- Produces: `StateStore` (`open(&Path)`, `open_with_io(&Path, Box<dyn ReplacementIo + Send>)`, `snapshot() -> AppState`, `publication_recovery_unresolved() -> Option<String>`), `OpenOutcome { Ready { store, report }, BlockedNewerSchema { found, supported } }`, `OpenReport { FirstLaunch, Loaded, Quarantined { quarantined_to } }`, `OpenError { detail }`, `STATE_FILE: &str = "state.json"`. The composition root supplies the directory; nothing else addresses the file.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/state/store.rs` with only this test module and register it:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{AppState, CURRENT_SCHEMA};
    use std::fs;
    use tempfile::TempDir;

    fn ready(outcome: OpenOutcome) -> (StateStore, OpenReport) {
        match outcome {
            OpenOutcome::Ready { store, report } => (store, report),
            OpenOutcome::BlockedNewerSchema { .. } => panic!("unexpectedly blocked"),
        }
    }

    #[test]
    fn absent_state_begins_first_launch_and_persists_defaults() {
        let dir = TempDir::new().unwrap();
        let (store, report) = ready(StateStore::open(dir.path()).unwrap());
        assert_eq!(report, OpenReport::FirstLaunch);
        assert_eq!(store.snapshot(), AppState::first_launch());
        drop(store);

        let (reopened, report) = ready(StateStore::open(dir.path()).unwrap());
        assert_eq!(report, OpenReport::Loaded);
        assert_eq!(reopened.snapshot(), AppState::first_launch());
    }

    #[test]
    fn newer_schema_blocks_without_any_write() {
        let dir = TempDir::new().unwrap();
        let newer = br#"{"schema":99,"future_field":true}"#;
        fs::write(dir.path().join(STATE_FILE), newer).unwrap();
        match StateStore::open(dir.path()).unwrap() {
            OpenOutcome::BlockedNewerSchema { found, supported } => {
                assert_eq!(found, 99);
                assert_eq!(supported, CURRENT_SCHEMA);
            }
            OpenOutcome::Ready { .. } => panic!("newer schema must block"),
        }
        // The file is byte-identical and nothing else appeared beside it.
        assert_eq!(fs::read(dir.path().join(STATE_FILE)).unwrap(), newer);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn malformed_state_is_quarantined_with_original_bytes_preserved() {
        let dir = TempDir::new().unwrap();
        let garbage = b"{not json";
        fs::write(dir.path().join(STATE_FILE), garbage).unwrap();

        let (store, report) = ready(StateStore::open(dir.path()).unwrap());
        let OpenReport::Quarantined { quarantined_to } = &report else {
            panic!("expected quarantine, got {report:?}");
        };
        // Original bytes preserved under the diagnostic name, defaults persisted,
        // and the unresolved-recovery notice survives in the new document.
        assert_eq!(fs::read(quarantined_to).unwrap(), garbage);
        let name = quarantined_to.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("state.json.quarantine-"));
        assert_eq!(
            store.publication_recovery_unresolved(),
            Some(name.to_owned())
        );
        drop(store);

        let (reopened, report) = ready(StateStore::open(dir.path()).unwrap());
        assert_eq!(report, OpenReport::Loaded);
        assert!(reopened.publication_recovery_unresolved().is_some());
    }

    #[test]
    fn unknown_lower_schema_is_quarantined_not_guessed() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(STATE_FILE), br#"{"schema":0}"#).unwrap();
        let (_store, report) = ready(StateStore::open(dir.path()).unwrap());
        assert!(matches!(report, OpenReport::Quarantined { .. }));
    }
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support state::store::`
Expected: compile error — `StateStore` not found.

- [ ] **Step 3: Implement**

Prepend above the test module:

```rust
//! The deep state store: one authoritative in-memory value, serialized mutations, and
//! schema-dispatched loading with quarantine recovery (docs/technical-design.md,
//! "Mutable state storage" and "State evolution and recovery").

use super::model::{AppState, CURRENT_SCHEMA};
use super::replace::{replace_state, RealIo, ReplaceOutcome, ReplacementIo};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const STATE_FILE: &str = "state.json";

pub struct StateStore {
    state_path: PathBuf,
    inner: Mutex<Inner>,
}

pub(super) struct Inner {
    pub(super) state: AppState,
    /// The exact authoritative bytes, kept so the next replacement can classify an
    /// ambiguous outcome against the true prior.
    pub(super) encoded: Vec<u8>,
    /// Set when reopen-validation failed; every further mutation is refused.
    pub(super) recovery_required: bool,
    pub(super) io_seam: Box<dyn ReplacementIo + Send>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OpenReport {
    FirstLaunch,
    Loaded,
    /// Unreadable state was moved aside and defaults persisted. Publication-reference
    /// recovery stays unresolved until the user restores the file or confirms discard.
    Quarantined { quarantined_to: PathBuf },
}

pub enum OpenOutcome {
    Ready { store: StateStore, report: OpenReport },
    /// Possibly valid data owned by a newer application version: never overwritten,
    /// never migrated down, no store constructed (docs/technical-design.md).
    BlockedNewerSchema { found: u32, supported: u32 },
}

#[derive(Debug)]
pub struct OpenError {
    pub detail: String,
}

#[derive(Deserialize)]
struct SchemaProbe {
    schema: u32,
}

impl StateStore {
    pub fn open(state_dir: &Path) -> Result<OpenOutcome, OpenError> {
        Self::open_with_io(state_dir, Box::new(RealIo))
    }

    pub fn open_with_io(
        state_dir: &Path,
        io_seam: Box<dyn ReplacementIo + Send>,
    ) -> Result<OpenOutcome, OpenError> {
        let state_path = state_dir.join(STATE_FILE);
        match fs::read(&state_path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let store =
                    Self::persist_fresh(state_path, AppState::first_launch(), io_seam)?;
                Ok(OpenOutcome::Ready { store, report: OpenReport::FirstLaunch })
            }
            Err(error) => Err(OpenError { detail: format!("reading state: {error}") }),
            Ok(bytes) => Self::open_existing(state_path, bytes, io_seam),
        }
    }

    fn open_existing(
        state_path: PathBuf,
        bytes: Vec<u8>,
        io_seam: Box<dyn ReplacementIo + Send>,
    ) -> Result<OpenOutcome, OpenError> {
        if let Ok(probe) = serde_json::from_slice::<SchemaProbe>(&bytes) {
            if probe.schema > CURRENT_SCHEMA {
                return Ok(OpenOutcome::BlockedNewerSchema {
                    found: probe.schema,
                    supported: CURRENT_SCHEMA,
                });
            }
            // No supported older schemas exist yet; a future migration dispatches on
            // probe.schema here and re-persists through the normal replacement path.
            if probe.schema == CURRENT_SCHEMA {
                if let Ok(state) = serde_json::from_slice::<AppState>(&bytes) {
                    let store = StateStore {
                        state_path,
                        inner: Mutex::new(Inner {
                            state,
                            encoded: bytes,
                            recovery_required: false,
                            io_seam,
                        }),
                    };
                    return Ok(OpenOutcome::Ready { store, report: OpenReport::Loaded });
                }
            }
        }
        Self::quarantine(state_path, bytes, io_seam)
    }

    /// Move the unreadable document to a diagnostic name and persist defaults that
    /// carry the unresolved-recovery notice. The unreadable file is never overwritten
    /// in place.
    fn quarantine(
        state_path: PathBuf,
        bytes: Vec<u8>,
        io_seam: Box<dyn ReplacementIo + Send>,
    ) -> Result<OpenOutcome, OpenError> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let hash = Sha256::digest(&bytes);
        let short: String = hash[..4].iter().map(|b| format!("{b:02x}")).collect();
        let name = format!("{STATE_FILE}.quarantine-{timestamp}-{short}");
        let quarantined_to = state_path.with_file_name(&name);
        fs::rename(&state_path, &quarantined_to)
            .map_err(|error| OpenError { detail: format!("quarantining state: {error}") })?;

        let mut defaults = AppState::first_launch();
        defaults.unresolved_quarantine = Some(name);
        let store = Self::persist_fresh(state_path, defaults, io_seam)?;
        Ok(OpenOutcome::Ready { store, report: OpenReport::Quarantined { quarantined_to } })
    }

    fn persist_fresh(
        state_path: PathBuf,
        state: AppState,
        mut io_seam: Box<dyn ReplacementIo + Send>,
    ) -> Result<StateStore, OpenError> {
        let encoded = encode(&state);
        match replace_state(io_seam.as_mut(), &state_path, &encoded, None) {
            ReplaceOutcome::Committed | ReplaceOutcome::CommittedDurabilityUncertain => {
                Ok(StateStore {
                    state_path,
                    inner: Mutex::new(Inner {
                        state,
                        encoded,
                        recovery_required: false,
                        io_seam,
                    }),
                })
            }
            ReplaceOutcome::PriorRetained { detail }
            | ReplaceOutcome::RecoveryRequired { detail } => Err(OpenError {
                detail: format!("persisting initial state: {detail}"),
            }),
        }
    }

    pub fn snapshot(&self) -> AppState {
        self.inner.lock().expect("state lock poisoned").state.clone()
    }

    pub fn publication_recovery_unresolved(&self) -> Option<String> {
        self.inner
            .lock()
            .expect("state lock poisoned")
            .state
            .unresolved_quarantine
            .clone()
    }

    pub(super) fn state_path(&self) -> &Path {
        &self.state_path
    }

    pub(super) fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("state lock poisoned")
    }
}

/// Human-readable: the state file is small, user-adjacent, and read during recovery.
pub(super) fn encode(state: &AppState) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(state).expect("state encodes to JSON");
    bytes.push(b'\n');
    bytes
}
```

- [ ] **Step 4: Run, pass, commit**

Run: `cargo test --features test-support state::store::` — expected: 4 passed.

```bash
git add src-tauri/src/state
git commit -m "Phase 1: state store open with quarantine and newer-schema block"
```

### Task 6: Typed mutations and the publication-reference capability

**Files:**
- Create: `src-tauri/src/state/mutations.rs`
- Modify: `src-tauri/src/state/mod.rs` (add `mod mutations;` and re-export: `pub use mutations::{MutationCommit, MutationError, PublicationError};`)

**Interfaces:**
- Consumes: Tasks 1, 3–5.
- Produces, all on `StateStore`: `add_discovery_location(PathBuf) -> Result<(DiscoveryLocationId, MutationCommit), MutationError>`; `rebind_discovery_location(DiscoveryLocationId, PathBuf) -> Result<MutationCommit, MutationError>`; `remove_discovery_location(DiscoveryLocationId) -> Result<MutationCommit, MutationError>` (cascades that location's publication references in the same mutation); `set_publication_reference(ModInstallationId, DiscoveryLocationId, expected_prior: Option<&RevisionId>, next: RevisionId) -> Result<MutationCommit, PublicationError>`; `confirm_discard_unrecovered_references() -> Result<MutationCommit, MutationError>`. Types: `MutationCommit { Committed, CommittedDurabilityUncertain }`, `MutationError { UnknownLocation, RecoveryRequired, StorageFailed { detail } }`, `PublicationError { ExpectedMismatch { actual: Option<RevisionId> }, Mutation(MutationError) }`. Phase 3's `revisions` uses exactly `set_publication_reference`; it can alter nothing else.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/state/mutations.rs` with only this test module and register it:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::path::LogicalPath;
    use crate::discovery::identity::ModInstallationId;
    use crate::state::model::RevisionId;
    use crate::state::replace::{RealIo, ReplacementIo};
    use crate::state::store::{OpenOutcome, StateStore, STATE_FILE};
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn open(dir: &Path) -> StateStore {
        match StateStore::open(dir).unwrap() {
            OpenOutcome::Ready { store, .. } => store,
            OpenOutcome::BlockedNewerSchema { .. } => panic!("blocked"),
        }
    }

    fn installation(store: &StateStore, location: DiscoveryLocationId) -> ModInstallationId {
        let _ = store;
        ModInstallationId::derive(location, &LogicalPath::parse("ugc_1").unwrap())
    }

    #[test]
    fn add_rebind_and_remove_locations_persist_across_reopen() {
        let dir = TempDir::new().unwrap();
        let store = open(dir.path());
        let (id, _) = store.add_discovery_location(PathBuf::from("/tmp/workshop")).unwrap();
        store
            .rebind_discovery_location(id, PathBuf::from("/tmp/moved-workshop"))
            .unwrap();
        drop(store);

        let store = open(dir.path());
        let locations = store.snapshot().discovery_locations;
        assert_eq!(locations.len(), 1);
        // A rebind preserves identity and changes only the path.
        assert_eq!(locations[0].id, id);
        assert_eq!(locations[0].path, PathBuf::from("/tmp/moved-workshop"));

        store.remove_discovery_location(id).unwrap();
        assert!(store.snapshot().discovery_locations.is_empty());
        assert!(matches!(
            store.rebind_discovery_location(id, PathBuf::from("/x")),
            Err(MutationError::UnknownLocation)
        ));
    }

    #[test]
    fn removing_a_location_cascades_only_its_publication_references() {
        let dir = TempDir::new().unwrap();
        let store = open(dir.path());
        let (kept, _) = store.add_discovery_location(PathBuf::from("/tmp/a")).unwrap();
        let (removed, _) = store.add_discovery_location(PathBuf::from("/tmp/b")).unwrap();
        let kept_install = installation(&store, kept);
        let removed_install = installation(&store, removed);
        store
            .set_publication_reference(kept_install, kept, None, RevisionId("r1".into()))
            .unwrap();
        store
            .set_publication_reference(removed_install, removed, None, RevisionId("r2".into()))
            .unwrap();

        store.remove_discovery_location(removed).unwrap();
        let refs = store.snapshot().publication_references;
        assert!(refs.contains_key(&kept_install));
        assert!(!refs.contains_key(&removed_install));
    }

    #[test]
    fn publication_reference_is_compare_and_swap() {
        let dir = TempDir::new().unwrap();
        let store = open(dir.path());
        let (location, _) = store.add_discovery_location(PathBuf::from("/tmp/a")).unwrap();
        let install = installation(&store, location);

        // Expecting a prior on an empty slot fails and reports the actual value.
        let mismatch = store.set_publication_reference(
            install,
            location,
            Some(&RevisionId("ghost".into())),
            RevisionId("r1".into()),
        );
        assert!(matches!(
            mismatch,
            Err(PublicationError::ExpectedMismatch { actual: None })
        ));

        store
            .set_publication_reference(install, location, None, RevisionId("r1".into()))
            .unwrap();
        // Wrong expected prior fails and reports the actual.
        let mismatch = store.set_publication_reference(
            install,
            location,
            Some(&RevisionId("r0".into())),
            RevisionId("r2".into()),
        );
        match mismatch {
            Err(PublicationError::ExpectedMismatch { actual }) => {
                assert_eq!(actual, Some(RevisionId("r1".into())));
            }
            other => panic!("expected mismatch, got {other:?}"),
        }
        // Matching expected prior replaces.
        store
            .set_publication_reference(
                install,
                location,
                Some(&RevisionId("r1".into())),
                RevisionId("r2".into()),
            )
            .unwrap();
        assert_eq!(
            store.snapshot().publication_references[&install].revision,
            RevisionId("r2".into())
        );
    }

    #[test]
    fn publication_reference_requires_a_known_location() {
        let dir = TempDir::new().unwrap();
        let store = open(dir.path());
        let unknown = DiscoveryLocationId::generate();
        let install = installation(&store, unknown);
        assert!(matches!(
            store.set_publication_reference(install, unknown, None, RevisionId("r".into())),
            Err(PublicationError::Mutation(MutationError::UnknownLocation))
        ));
    }

    #[test]
    fn a_failed_mutation_changes_neither_memory_nor_disk() {
        struct AlwaysFail;
        impl ReplacementIo for AlwaysFail {
            fn write_temp(&mut self, _d: &Path, _b: &[u8]) -> io::Result<PathBuf> {
                Err(io::Error::other("injected failure"))
            }
            fn rename(&mut self, _f: &Path, _t: &Path) -> io::Result<()> {
                unreachable!("write_temp already failed")
            }
            fn sync_dir(&mut self, _d: &Path) -> io::Result<()> {
                unreachable!("write_temp already failed")
            }
        }
        let dir = TempDir::new().unwrap();
        // Seed a valid file with real I/O, then reopen with the failing seam.
        drop(open(dir.path()));
        let before = fs::read(dir.path().join(STATE_FILE)).unwrap();
        let store = match StateStore::open_with_io(dir.path(), Box::new(AlwaysFail)).unwrap() {
            OpenOutcome::Ready { store, .. } => store,
            OpenOutcome::BlockedNewerSchema { .. } => panic!("blocked"),
        };
        let result = store.add_discovery_location(PathBuf::from("/tmp/a"));
        assert!(matches!(result, Err(MutationError::StorageFailed { .. })));
        assert!(store.snapshot().discovery_locations.is_empty());
        assert_eq!(fs::read(dir.path().join(STATE_FILE)).unwrap(), before);
    }

    #[test]
    fn recovery_required_poisons_all_further_mutations() {
        struct CorruptingRename;
        impl ReplacementIo for CorruptingRename {
            fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
                RealIo.write_temp(dir, bytes)
            }
            fn rename(&mut self, _f: &Path, to: &Path) -> io::Result<()> {
                fs::write(to, b"neither prior nor next").unwrap();
                Err(io::Error::other("injected rename failure"))
            }
            fn sync_dir(&mut self, _d: &Path) -> io::Result<()> {
                Ok(())
            }
        }
        let dir = TempDir::new().unwrap();
        drop(open(dir.path()));
        let store = match StateStore::open_with_io(dir.path(), Box::new(CorruptingRename))
            .unwrap()
        {
            OpenOutcome::Ready { store, .. } => store,
            OpenOutcome::BlockedNewerSchema { .. } => panic!("blocked"),
        };
        assert!(matches!(
            store.add_discovery_location(PathBuf::from("/tmp/a")),
            Err(MutationError::RecoveryRequired)
        ));
        // Poisoned: even a mutation that would use fresh I/O is refused.
        assert!(matches!(
            store.confirm_discard_unrecovered_references(),
            Err(MutationError::RecoveryRequired)
        ));
    }

    #[test]
    fn confirm_discard_clears_the_persisted_notice() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(STATE_FILE), b"{garbage").unwrap();
        let store = open(dir.path());
        assert!(store.publication_recovery_unresolved().is_some());
        store.confirm_discard_unrecovered_references().unwrap();
        assert_eq!(store.publication_recovery_unresolved(), None);
        drop(store);
        assert_eq!(open(dir.path()).publication_recovery_unresolved(), None);
    }
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support state::mutations::`
Expected: compile error — the mutation methods do not exist.

- [ ] **Step 3: Implement**

Prepend above the test module:

```rust
//! Typed mutations on the state store. The public surface is per-owner and narrow:
//! Discovery Location configuration for setup workflows, and the publication-reference
//! compare-and-swap that is the only mutation `revisions` receives
//! (docs/technical-design.md, "Mutable state storage"). No caller gets the document.

use super::model::{DiscoveryLocation, PublicationReference, RevisionId};
use super::replace::{replace_state, ReplaceOutcome};
use super::store::{encode, StateStore};
use crate::discovery::identity::{DiscoveryLocationId, ModInstallationId};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationCommit {
    Committed,
    /// Visible but durability unconfirmed; surfaced so workflows can report it.
    CommittedDurabilityUncertain,
}

#[derive(Debug)]
pub enum MutationError {
    UnknownLocation,
    /// The store observed an unresolvable replacement outcome; mutation is stopped
    /// until state recovery resolves it.
    RecoveryRequired,
    /// Failure before the commit point: memory and disk both retain the prior state.
    StorageFailed { detail: String },
}

#[derive(Debug)]
pub enum PublicationError {
    /// The stored reference did not match the expected prior; nothing changed.
    ExpectedMismatch { actual: Option<RevisionId> },
    Mutation(MutationError),
}

impl StateStore {
    pub fn add_discovery_location(
        &self,
        path: PathBuf,
    ) -> Result<(DiscoveryLocationId, MutationCommit), MutationError> {
        let id = DiscoveryLocationId::generate();
        let commit = self.commit(|state| {
            state.discovery_locations.push(DiscoveryLocation { id, path });
            Ok(())
        })?;
        Ok((id, commit))
    }

    /// Explicitly a rebind of the same configured location: identity and derived
    /// installation identities are preserved; only the path changes.
    pub fn rebind_discovery_location(
        &self,
        id: DiscoveryLocationId,
        new_path: PathBuf,
    ) -> Result<MutationCommit, MutationError> {
        self.commit(|state| {
            let location = state
                .discovery_locations
                .iter_mut()
                .find(|l| l.id == id)
                .ok_or(MutationError::UnknownLocation)?;
            location.path = new_path;
            Ok(())
        })
    }

    /// Confirmed-intent removal: the location and its publication references leave in
    /// one mutation (docs/technical-design.md, "Unavailable and removed Discovery
    /// Locations"). Bundle deletion is later cleanup, not state's concern.
    pub fn remove_discovery_location(
        &self,
        id: DiscoveryLocationId,
    ) -> Result<MutationCommit, MutationError> {
        self.commit(|state| {
            let before = state.discovery_locations.len();
            state.discovery_locations.retain(|l| l.id != id);
            if state.discovery_locations.len() == before {
                return Err(MutationError::UnknownLocation);
            }
            state
                .publication_references
                .retain(|_, reference| reference.location != id);
            Ok(())
        })
    }

    /// The narrow capability `revisions` consumes in Phase 3.
    pub fn set_publication_reference(
        &self,
        installation: ModInstallationId,
        location: DiscoveryLocationId,
        expected_prior: Option<&RevisionId>,
        next: RevisionId,
    ) -> Result<MutationCommit, PublicationError> {
        self.commit(|state| {
            if !state.discovery_locations.iter().any(|l| l.id == location) {
                return Err(PublicationError::Mutation(MutationError::UnknownLocation));
            }
            let actual = state
                .publication_references
                .get(&installation)
                .map(|reference| reference.revision.clone());
            if actual.as_ref() != expected_prior {
                return Err(PublicationError::ExpectedMismatch { actual });
            }
            state.publication_references.insert(
                installation,
                PublicationReference { location, revision: next.clone() },
            );
            Ok(())
        })
        .map_err(|error| match error {
            CommitError::Caller(publication) => publication,
            CommitError::Mutation(mutation) => PublicationError::Mutation(mutation),
        })
    }

    pub fn confirm_discard_unrecovered_references(
        &self,
    ) -> Result<MutationCommit, MutationError> {
        self.commit(|state| {
            state.unresolved_quarantine = None;
            Ok(())
        })
    }
}

/// Distinguishes a caller-typed refusal from the store's own storage outcomes so
/// `commit` can serve both error unions without flattening them.
enum CommitError<E> {
    Caller(E),
    Mutation(MutationError),
}

impl<E> From<MutationError> for CommitError<E> {
    fn from(error: MutationError) -> Self {
        CommitError::Mutation(error)
    }
}

impl StateStore {
    fn commit<E>(
        &self,
        apply: impl FnOnce(&mut super::model::AppState) -> Result<(), E>,
    ) -> Result<MutationCommit, CommitErrorFor<E>>
    where
        CommitErrorFor<E>: From<MutationError> + FromCaller<E>,
    {
        let mut inner = self.lock();
        if inner.recovery_required {
            return Err(CommitErrorFor::<E>::from(MutationError::RecoveryRequired));
        }
        let mut next = inner.state.clone();
        apply(&mut next).map_err(CommitErrorFor::<E>::from_caller)?;
        let next_bytes = encode(&next);
        let prior_bytes = inner.encoded.clone();
        let outcome = replace_state(
            inner.io_seam.as_mut(),
            self.state_path(),
            &next_bytes,
            Some(&prior_bytes),
        );
        match outcome {
            ReplaceOutcome::Committed => {
                inner.state = next;
                inner.encoded = next_bytes;
                Ok(MutationCommit::Committed)
            }
            ReplaceOutcome::CommittedDurabilityUncertain => {
                inner.state = next;
                inner.encoded = next_bytes;
                Ok(MutationCommit::CommittedDurabilityUncertain)
            }
            ReplaceOutcome::PriorRetained { detail } => {
                Err(CommitErrorFor::<E>::from(MutationError::StorageFailed { detail }))
            }
            ReplaceOutcome::RecoveryRequired { .. } => {
                inner.recovery_required = true;
                Err(CommitErrorFor::<E>::from(MutationError::RecoveryRequired))
            }
        }
    }
}

/// Maps a caller error type to the union `commit` returns for it. `MutationError`
/// callers collapse both channels into `MutationError`; `PublicationError` callers keep
/// their typed refusals distinct.
trait FromCaller<E> {
    fn from_caller(error: E) -> Self;
}

type CommitErrorFor<E> = <E as CommitChannel>::Union;

trait CommitChannel: Sized {
    type Union: From<MutationError> + FromCaller<Self>;
}

impl CommitChannel for MutationError {
    type Union = MutationError;
}

impl CommitChannel for PublicationError {
    type Union = CommitError<PublicationError>;
}

impl FromCaller<MutationError> for MutationError {
    fn from_caller(error: MutationError) -> Self {
        error
    }
}

impl FromCaller<PublicationError> for CommitError<PublicationError> {
    fn from_caller(error: PublicationError) -> Self {
        CommitError::Caller(error)
    }
}
```

> **Implementer note:** the `CommitChannel` machinery above is the plan's sketch for
> serving two error unions from one `commit`. If it fights the borrow checker or reads
> worse than the duplication it removes, the sanctioned simpler alternative is two
> private methods — `commit_mutation` returning `MutationError` and
> `commit_publication` returning `PublicationError` — sharing the replacement block via
> a third private fn. Behavior and the public surface must match the tests exactly;
> the internal shape is yours.

- [ ] **Step 4: Run, pass, commit**

Run: `cargo test --features test-support state::mutations::` — expected: 7 passed.

```bash
git add src-tauri/src/state
git commit -m "Phase 1: typed state mutations and publication-reference capability"
```

### Task 7: Module surface and full-gate check

**Files:**
- Modify: `src-tauri/src/state/mod.rs` (final form below)

- [ ] **Step 1: Settle the module surface**

`src-tauri/src/state/mod.rs`:

```rust
//! Deep owner of the durable mutable state document: schema, atomic replacement,
//! quarantine recovery, and the narrow publication-reference capability
//! (docs/technical-design.md, "Mutable state storage"). The composition root supplies
//! the application-data directory; no other module addresses the state file.

pub mod model;
mod mutations;
pub mod replace;
pub mod store;

pub use model::{AppState, DiscoveryLocation, PublicationReference, RevisionId, CURRENT_SCHEMA};
pub use mutations::{MutationCommit, MutationError, PublicationError};
pub use store::{OpenOutcome, OpenReport, StateStore, STATE_FILE};
```

- [ ] **Step 2: Run the full gate**

Run: `tools/ci/check.sh` — expected exit 0 (all state suites green alongside Phase 0's).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/state
git commit -m "Phase 1: state module surface"
```

---

## Ticket C — Discovery (plan Tasks 8–11)

### Task 8: Descriptor metadata reader

**Files:**
- Create: `src-tauri/src/discovery/descriptor.rs`
- Modify: `src-tauri/src/discovery/mod.rs` (add `mod descriptor;` and `pub use descriptor::DescriptorMetadata;`)

**Interfaces:**
- Produces: `DescriptorMetadata { name, version, supported_version, tags, remote_file_id }` (all `Option`/`Vec`) and `parse_descriptor(&str) -> DescriptorMetadata`. Advisory only — Task 9 attaches it to installations; analysis never consumes it.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/discovery/descriptor.rs` with only this test module and register it:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_observed_descriptor_fields() {
        let text = r#"
name="Gigastructural Engineering & More"
version="3.44.*"
tags={
	"Technologies"
	"Gameplay"
}
supported_version="4.4.*"
remote_file_id="1121692237"
path="/some/absolute/path"
"#;
        let metadata = parse_descriptor(text);
        assert_eq!(metadata.name.as_deref(), Some("Gigastructural Engineering & More"));
        assert_eq!(metadata.version.as_deref(), Some("3.44.*"));
        assert_eq!(metadata.supported_version.as_deref(), Some("4.4.*"));
        assert_eq!(metadata.remote_file_id.as_deref(), Some("1121692237"));
        assert_eq!(metadata.tags, vec!["Technologies", "Gameplay"]);
    }

    #[test]
    fn tolerates_unknown_keys_malformed_lines_and_single_line_lists() {
        let text = "picture=\"thumb.png\"\nname=unquoted junk\ntags={ \"AI\" }\nname=\"Real\"";
        let metadata = parse_descriptor(text);
        // Malformed `name=unquoted junk` is skipped; the later valid line wins.
        assert_eq!(metadata.name.as_deref(), Some("Real"));
        assert_eq!(metadata.tags, vec!["AI"]);
    }

    #[test]
    fn empty_input_is_empty_metadata() {
        assert_eq!(parse_descriptor(""), DescriptorMetadata::default());
    }
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support discovery::descriptor::`
Expected: compile error — `parse_descriptor` not found.

- [ ] **Step 3: Implement**

Prepend above the test module:

```rust
//! Minimal reader for `.mod` descriptor metadata. Advisory display data only:
//! discovery "reads only the metadata needed to populate the Mod Library"
//! (docs/technical-design.md, "Source module"). The real Clausewitz parser arrives in
//! Phase 4 and analysis never consumes this reader, so a tolerant line scanner is
//! proportionate here and a shared parser dependency is not.

/// Observed `.mod` fields (AGENTS.md, "Mod activation"). Everything optional; absence
/// and malformation are advisory facts, never scan failures.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DescriptorMetadata {
    pub name: Option<String>,
    pub version: Option<String>,
    pub supported_version: Option<String>,
    pub tags: Vec<String>,
    pub remote_file_id: Option<String>,
}

pub fn parse_descriptor(text: &str) -> DescriptorMetadata {
    let mut metadata = DescriptorMetadata::default();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key == "tags" {
            metadata.tags = parse_tags(value, &mut lines);
            continue;
        }
        let Some(unquoted) = unquote(value) else {
            continue;
        };
        match key {
            "name" => metadata.name = Some(unquoted),
            "version" => metadata.version = Some(unquoted),
            "supported_version" => metadata.supported_version = Some(unquoted),
            "remote_file_id" => metadata.remote_file_id = Some(unquoted),
            _ => {}
        }
    }
    metadata
}

/// `tags={ "a" "b" }` on one line, or `tags={` followed by one quoted tag per line
/// until `}` — both observed layouts.
fn parse_tags<'a>(value: &str, lines: &mut impl Iterator<Item = &'a str>) -> Vec<String> {
    let Some(open) = value.strip_prefix('{') else {
        return Vec::new();
    };
    let mut tags = Vec::new();
    let mut collect = |segment: &str| {
        let mut rest = segment;
        while let Some(start) = rest.find('"') {
            let Some(len) = rest[start + 1..].find('"') else {
                return;
            };
            tags.push(rest[start + 1..start + 1 + len].to_owned());
            rest = &rest[start + len + 2..];
        }
    };
    if let Some(inline) = open.split('}').next().filter(|_| open.contains('}')) {
        collect(inline);
        return tags;
    }
    collect(open);
    for line in lines {
        if line.trim_start().starts_with('}') {
            break;
        }
        collect(line);
    }
    tags
}

fn unquote(value: &str) -> Option<String> {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .map(str::to_owned)
}
```

- [ ] **Step 4: Run, pass, commit**

Run: `cargo test --features test-support discovery::descriptor::` — expected: 3 passed.

```bash
git add src-tauri/src/discovery
git commit -m "Phase 1: advisory descriptor metadata reader"
```

### Task 9: Location scanning with visible collisions

**Files:**
- Modify: `src-tauri/src/discovery/mod.rs`

**Interfaces:**
- Consumes: Tasks 1, 8; `canonical::path::LogicalPath`.
- Produces: `ConfiguredLocation { id, path }`, `LocationScan { location, outcome }`, `LocationOutcome { Available(LocationContents), Unavailable { reason } }`, `LocationContents { installations, collisions, rejected }`, `ModInstallation { id, location, relative_path, metadata }`, `PathCollision { logical, raw_names }`, `RejectedEntry { raw_name, reason }`, and `scan_location(&ConfiguredLocation) -> LocationScan`. Phase 3's application layer maps stored `state::DiscoveryLocation` records into `ConfiguredLocation` values.

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/src/discovery/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use identity::DiscoveryLocationId;
    use std::fs;
    use tempfile::TempDir;

    fn location(path: &std::path::Path) -> ConfiguredLocation {
        ConfiguredLocation { id: DiscoveryLocationId::generate(), path: path.to_path_buf() }
    }

    #[test]
    fn scans_child_directories_as_installations_with_descriptor_metadata() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("ugc_123")).unwrap();
        fs::write(
            dir.path().join("ugc_123/descriptor.mod"),
            "name=\"Some Mod\"\nsupported_version=\"4.4.*\"\n",
        )
        .unwrap();
        fs::create_dir(dir.path().join("bare_mod")).unwrap();
        // Root-level files are not installations.
        fs::write(dir.path().join("stray.mod"), "name=\"Stray\"\n").unwrap();

        let configured = location(dir.path());
        let scan = scan_location(&configured);
        let LocationOutcome::Available(contents) = scan.outcome else {
            panic!("expected available");
        };
        assert_eq!(contents.installations.len(), 2);
        let with_meta = contents
            .installations
            .iter()
            .find(|i| i.relative_path.as_str() == "ugc_123")
            .unwrap();
        assert_eq!(
            with_meta.metadata.as_ref().unwrap().name.as_deref(),
            Some("Some Mod")
        );
        assert_eq!(
            with_meta.id,
            identity::ModInstallationId::derive(
                configured.id,
                &crate::canonical::path::LogicalPath::parse("ugc_123").unwrap()
            )
        );
        let bare = contents
            .installations
            .iter()
            .find(|i| i.relative_path.as_str() == "bare_mod")
            .unwrap();
        assert!(bare.metadata.is_none());
        assert!(contents.collisions.is_empty());
    }

    #[test]
    fn installations_are_ordered_by_normalized_path_bytes() {
        let dir = TempDir::new().unwrap();
        for name in ["zeta", "Alpha", "midway"] {
            fs::create_dir(dir.path().join(name)).unwrap();
        }
        let scan = scan_location(&location(dir.path()));
        let LocationOutcome::Available(contents) = scan.outcome else {
            panic!("expected available");
        };
        let order: Vec<&str> = contents
            .installations
            .iter()
            .map(|i| i.relative_path.as_str())
            .collect();
        // Byte order: uppercase before lowercase; never filesystem walk order.
        assert_eq!(order, vec!["Alpha", "midway", "zeta"]);
    }

    #[test]
    fn an_unreadable_location_is_unavailable_not_empty() {
        let dir = TempDir::new().unwrap();
        let gone = dir.path().join("never-created");
        let scan = scan_location(&location(&gone));
        assert!(matches!(scan.outcome, LocationOutcome::Unavailable { .. }));
    }

    // Collisions cannot be created on APFS (case- and normalization-insensitive), so
    // the classification rule is tested at its pure seam with raw names.
    #[test]
    fn entries_normalizing_to_one_logical_path_are_a_visible_collision() {
        let owner = DiscoveryLocationId::generate();
        let contents = classify_entries(
            owner,
            vec![
                RawEntry::Directory("te\u{301}ch_mod".to_owned()),
                RawEntry::Directory("t\u{e9}ch_mod".to_owned()),
                RawEntry::Directory("unrelated".to_owned()),
            ],
        );
        assert_eq!(contents.installations.len(), 1);
        assert_eq!(contents.installations[0].relative_path.as_str(), "unrelated");
        assert_eq!(contents.collisions.len(), 1);
        assert_eq!(contents.collisions[0].raw_names.len(), 2);
    }

    #[test]
    fn invalid_names_are_rejected_entries_not_silent_omissions() {
        let owner = DiscoveryLocationId::generate();
        let contents = classify_entries(
            owner,
            vec![
                RawEntry::InvalidUnicode("bad\u{fffd}name".to_owned()),
                RawEntry::Directory("fine".to_owned()),
            ],
        );
        assert_eq!(contents.installations.len(), 1);
        assert_eq!(contents.rejected.len(), 1);
    }
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support discovery::tests::`
Expected: compile error — `ConfiguredLocation` not found.

- [ ] **Step 3: Implement**

Insert into `src-tauri/src/discovery/mod.rs` between the module declarations and the test module:

```rust
use crate::canonical::path::LogicalPath;
use descriptor::parse_descriptor;
use identity::{DiscoveryLocationId, ModInstallationId};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// A user-confirmed Discovery Location as the scan input. The application layer maps
/// stored `state::DiscoveryLocation` records into these; `discovery` never reads state.
#[derive(Debug, Clone)]
pub struct ConfiguredLocation {
    pub id: DiscoveryLocationId,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct LocationScan {
    pub location: DiscoveryLocationId,
    pub outcome: LocationOutcome,
}

#[derive(Debug)]
pub enum LocationOutcome {
    Available(LocationContents),
    /// A failed scan reports the location unavailable rather than empty; stored
    /// configuration and revisions are never dropped for it
    /// (docs/technical-design.md, "Unavailable and removed Discovery Locations").
    Unavailable { reason: String },
}

#[derive(Debug, Default)]
pub struct LocationContents {
    /// Ordered by normalized logical-path bytes, never filesystem walk order.
    pub installations: Vec<ModInstallation>,
    pub collisions: Vec<PathCollision>,
    pub rejected: Vec<RejectedEntry>,
}

#[derive(Debug)]
pub struct ModInstallation {
    pub id: ModInstallationId,
    pub location: DiscoveryLocationId,
    pub relative_path: LogicalPath,
    /// Advisory descriptor metadata; absence is normal.
    pub metadata: Option<descriptor::DescriptorMetadata>,
}

/// Distinct raw entries normalizing to one logical path. Neither becomes an
/// installation: an arbitrary winner would be silent data loss.
#[derive(Debug)]
pub struct PathCollision {
    pub logical: LogicalPath,
    pub raw_names: Vec<String>,
}

#[derive(Debug)]
pub struct RejectedEntry {
    /// Lossy rendering for the human-facing report only; never identity.
    pub raw_name: String,
    pub reason: String,
}

/// A directory entry as enumeration observed it, before classification.
#[derive(Debug)]
pub enum RawEntry {
    Directory(String),
    /// The name could not be decoded as Unicode; carried as a lossy label.
    InvalidUnicode(String),
}

pub fn scan_location(location: &ConfiguredLocation) -> LocationScan {
    let entries = match fs::read_dir(&location.path) {
        Ok(entries) => entries,
        Err(error) => {
            return LocationScan {
                location: location.id,
                outcome: LocationOutcome::Unavailable { reason: error.to_string() },
            };
        }
    };
    let mut raw = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        match name.to_str() {
            Some(text) => raw.push(RawEntry::Directory(text.to_owned())),
            None => raw.push(RawEntry::InvalidUnicode(name.to_string_lossy().into_owned())),
        }
    }
    let mut contents = classify_entries(location.id, raw);
    for installation in &mut contents.installations {
        // Filesystem access uses the raw name, which for surviving installations is
        // byte-identical to the logical path (a normalization-changing name would have
        // classified differently only alongside a collision partner; the logical form
        // still addresses it on macOS's normalization-insensitive filesystems).
        let descriptor_path = location
            .path
            .join(installation.relative_path.as_str())
            .join("descriptor.mod");
        if let Ok(text) = fs::read_to_string(descriptor_path) {
            installation.metadata = Some(parse_descriptor(&text));
        }
    }
    LocationScan { location: location.id, outcome: LocationOutcome::Available(contents) }
}

/// Pure classification seam: collision and rejection rules are testable without a
/// filesystem, which macOS (case- and normalization-insensitive APFS) cannot stage.
pub fn classify_entries(
    location: DiscoveryLocationId,
    raw_entries: Vec<RawEntry>,
) -> LocationContents {
    let mut by_logical: BTreeMap<LogicalPath, Vec<String>> = BTreeMap::new();
    let mut rejected = Vec::new();
    for entry in raw_entries {
        match entry {
            RawEntry::Directory(name) => match LogicalPath::parse(&name) {
                Ok(logical) => by_logical.entry(logical).or_default().push(name),
                Err(error) => rejected.push(RejectedEntry {
                    raw_name: name,
                    reason: format!("invalid mod directory name: {error:?}"),
                }),
            },
            RawEntry::InvalidUnicode(label) => rejected.push(RejectedEntry {
                raw_name: label,
                reason: "directory name is not valid Unicode".to_owned(),
            }),
        }
    }
    let mut contents = LocationContents { rejected, ..LocationContents::default() };
    for (logical, raw_names) in by_logical {
        if raw_names.len() > 1 {
            contents.collisions.push(PathCollision { logical, raw_names });
        } else {
            contents.installations.push(ModInstallation {
                id: ModInstallationId::derive(location, &logical),
                location,
                relative_path: logical,
                metadata: None,
            });
        }
    }
    contents
}
```

- [ ] **Step 4: Run, pass, commit**

Run: `cargo test --features test-support discovery::` — expected: all discovery suites pass.

```bash
git add src-tauri/src/discovery
git commit -m "Phase 1: location scanning with visible collisions"
```

### Task 10: First-run proposals and Stellaris install metadata

**Files:**
- Create: `src-tauri/src/discovery/proposals.rs`
- Modify: `src-tauri/src/discovery/mod.rs` (add `pub mod proposals;`)

**Interfaces:**
- Produces: `ProposedLocations { workshop_mods: Option<PathBuf>, local_mods: Option<PathBuf> }`, `propose_locations(home: &Path) -> ProposedLocations`; `StellarisInstall { root, version, raw_version, mods_compatibility_version }`, `detect_stellaris_install(home: &Path) -> Option<StellarisInstall>`. `home` is a parameter so tests build fake trees; the composition root supplies the real home directory. macOS paths only (functional MVP target); other platforms are added with their release work.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/discovery/proposals.rs` with only this test module and register it:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn proposes_only_locations_that_exist() {
        let home = TempDir::new().unwrap();
        let workshop = home
            .path()
            .join("Library/Application Support/Steam/steamapps/workshop/content/281990");
        fs::create_dir_all(&workshop).unwrap();

        let proposals = propose_locations(home.path());
        assert_eq!(proposals.workshop_mods, Some(workshop));
        // The local mod directory was never created, so it is not proposed.
        assert_eq!(proposals.local_mods, None);
    }

    #[test]
    fn reads_launcher_settings_for_the_installed_build() {
        let home = TempDir::new().unwrap();
        let install = home
            .path()
            .join("Library/Application Support/Steam/steamapps/common/Stellaris");
        fs::create_dir_all(&install).unwrap();
        fs::write(
            install.join("launcher-settings.json"),
            r#"{"version":"Pegasus v4.4.6","rawVersion":"4.4.6","modsCompatibilityVersion":"4.4","exePath":"stellaris.app/Contents/MacOS/stellaris"}"#,
        )
        .unwrap();

        let found = detect_stellaris_install(home.path()).unwrap();
        assert_eq!(found.root, install);
        assert_eq!(found.version.as_deref(), Some("Pegasus v4.4.6"));
        assert_eq!(found.raw_version.as_deref(), Some("4.4.6"));
        assert_eq!(found.mods_compatibility_version.as_deref(), Some("4.4"));
    }

    #[test]
    fn an_install_without_readable_settings_is_still_detected() {
        let home = TempDir::new().unwrap();
        let install = home
            .path()
            .join("Library/Application Support/Steam/steamapps/common/Stellaris");
        fs::create_dir_all(&install).unwrap();
        let found = detect_stellaris_install(home.path()).unwrap();
        assert_eq!(found.root, install);
        assert_eq!(found.version, None);
    }

    #[test]
    fn no_install_directory_means_no_detection() {
        let home = TempDir::new().unwrap();
        assert!(detect_stellaris_install(home.path()).is_none());
    }
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support discovery::proposals::`
Expected: compile error — `propose_locations` not found.

- [ ] **Step 3: Implement**

Prepend above the test module:

```rust
//! First-run Discovery Location proposals and Stellaris install detection
//! (docs/technical-design.md: setup "detects proposed Discovery Locations but allows
//! user correction"). Everything here is a proposal for the user to confirm or edit —
//! nothing is a hard-coded product path. macOS Steam defaults only; other platforms
//! arrive with their release work (AGENTS.md, "Stellaris environment reference").

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub struct ProposedLocations {
    pub workshop_mods: Option<PathBuf>,
    pub local_mods: Option<PathBuf>,
}

pub fn propose_locations(home: &Path) -> ProposedLocations {
    let workshop = home
        .join("Library/Application Support/Steam/steamapps/workshop/content/281990");
    let local = home.join("Documents/Paradox Interactive/Stellaris/mod");
    ProposedLocations {
        workshop_mods: workshop.is_dir().then_some(workshop),
        local_mods: local.is_dir().then_some(local),
    }
}

/// `launcher-settings.json` is authoritative for the installed build; the game.log
/// banner reflects only the last run and is deliberately not read here (AGENTS.md,
/// "Version pinning").
#[derive(Debug)]
pub struct StellarisInstall {
    pub root: PathBuf,
    pub version: Option<String>,
    pub raw_version: Option<String>,
    pub mods_compatibility_version: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct LauncherSettings {
    version: Option<String>,
    raw_version: Option<String>,
    mods_compatibility_version: Option<String>,
}

pub fn detect_stellaris_install(home: &Path) -> Option<StellarisInstall> {
    let root = home.join("Library/Application Support/Steam/steamapps/common/Stellaris");
    if !root.is_dir() {
        return None;
    }
    let settings: LauncherSettings = fs::read(root.join("launcher-settings.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    Some(StellarisInstall {
        root,
        version: settings.version,
        raw_version: settings.raw_version,
        mods_compatibility_version: settings.mods_compatibility_version,
    })
}
```

- [ ] **Step 4: Run, pass, commit**

Run: `cargo test --features test-support discovery::proposals::` — expected: 4 passed.

```bash
git add src-tauri/src/discovery
git commit -m "Phase 1: first-run proposals and install detection"
```

### Task 11: Record Phase 1 decisions

**Files:**
- Modify: `docs/decision-log.md` (append, matching the existing entry format)
- Modify: `docs/implementation-plan.md` (mark Phase 1 implemented, link this plan)

- [ ] **Step 1: Append decision-log entries**

Record the four design decisions from this plan's header: the stored `{ location, revision }` publication-reference shape and why the location component is necessary; the persisted `unresolved_quarantine` notice; child-directories-only installation rule with root descriptors as advisory metadata; and the pure collision-classification seam forced by APFS insensitivity.

- [ ] **Step 2: Update the outline and run the full gate**

Change the Phase 1 heading in `docs/implementation-plan.md` to `## Phase 1 — Durable state and discovery (implemented — [detailed plan](./plans/phase-1-state-and-discovery.md))`.

Run: `tools/ci/check.sh` — expected exit 0.

- [ ] **Step 3: Commit**

```bash
git add docs/decision-log.md docs/implementation-plan.md
git commit -m "Phase 1: record state and discovery decisions"
```

---

## Self-review

- **Outline coverage:** outline item 1 (state module, replacement, `CommittedDurabilityUncertain`, serialized mutations) → Tasks 3–5; item 2 (quarantine, recovery states, newer-schema block) → Task 5 + the poisoning/discard tests in Task 6; item 3 (publication-reference capability) → Task 6; item 4 (location identity, rebind vs. remove) → Tasks 1, 6; item 5 (installation identity, collision visibility) → Tasks 1–2, 9; item 6 (scanning, descriptor metadata, unavailable behavior) → Tasks 8–10. Exit criteria: crash-injection per replacement step (Task 4, plus mutation-level injection in Task 6); identity property tests (Tasks 1–2); discovery behavioral tests over fixture trees (Tasks 9–10).
- **Placeholders:** none; every step carries complete code or exact commands. The one sanctioned flexibility (Task 6's `CommitChannel` machinery) names its concrete simpler alternative and pins the public surface via tests.
- **Type consistency:** `DiscoveryLocationId`/`ModInstallationId` signatures match across Tasks 1–2, 3, 6, 9; `RevisionId`/`PublicationReference` match between Tasks 3 and 6; `ReplaceOutcome` variants match between Tasks 4 and 6; `encode`/`state_path`/`lock` visibility (`pub(super)`) serves `mutations.rs` from `store.rs`.
- **Known risks, accepted:** `Cow<str>` deserialize in Task 2 may need plain `String` depending on serde's map-key deserializer — behavior, not representation, is the contract; Task 6's generic commit plumbing is the most likely place implementer judgment improves on the plan (explicitly sanctioned); quarantine timestamps use wall-clock seconds, which is fine because quarantine names are diagnostic, not identity.
