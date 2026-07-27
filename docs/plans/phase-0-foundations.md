# Phase 0 — Foundations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A compiling module skeleton with the shared primitives every later phase consumes — canonical encoding, logical paths, exact numerics, error conventions, the analysis version vector — plus test and CI infrastructure with proven negative controls.

**Architecture:** All Rust work happens in the existing `src-tauri` package (`stellaris_docs_lib`). The accepted module map lands as documented skeleton modules; two new leaf modules, `canonical` and `error`, hold shared primitives that sit below the deep-module row. No behavior beyond the primitives themselves; no filesystem, parser, or Tauri surface beyond the scaffold.

**Tech Stack:** Rust 1.97.1 (pinned), Tauri 2, `sha2`, `unicode-normalization`, `num-bigint`/`num-rational`/`num-traits`, `uuid`, `proptest`, `tempfile`; Vitest for the frontend harness; GitHub Actions on `macos-latest`.

## Global Constraints

- One Cargo package, one Rust process; the executable target only starts the library (`docs/technical-design.md`, "Rust package and dependency direction").
- Application modules never import Tauri or an HTTP framework; framework types stay in `composition` and `transport`.
- Stable identities are SHA-256 over domain-separated, tagged, length-prefixed canonical encodings; never serializer output or map iteration order.
- Logical paths: `/` separators, Unicode NFC, exact case-preserving comparison, no `.`/`..`, ordering by normalized UTF-8 bytes, invalid Unicode rejected without lossy conversion.
- Binary floating point never participates in source equality, hashes, stable identity, or displayed exact values.
- Panics are defects, never control flow; expected outcomes are typed `Result` errors; unexpected failures carry correlation identifiers.
- Tests and development tools use isolated temporary application-data directories (single-instance caller precondition).
- TDD: watch each test fail before making it pass; negative controls prove gates can go red.

**Design deviations to record (Task 10):** `canonical` and `error` are top-level leaf modules not named in the technical design's module map; the Rust edition is bumped to 2024 while the crate is empty; the toolchain is pinned at 1.97.1 for reproducibility.

---

### Task 1: Toolchain pin, dependencies, and module skeleton

**Files:**
- Create: `rust-toolchain.toml` (repo root)
- Modify: `src-tauri/Cargo.toml` (full replacement below)
- Modify: `src-tauri/src/lib.rs` (full replacement below)
- Create: `src-tauri/src/composition/mod.rs`
- Create: `src-tauri/src/transport/mod.rs`, `src-tauri/src/transport/tauri.rs`, `src-tauri/src/transport/http.rs`
- Create: `src-tauri/src/application/mod.rs`, `src-tauri/src/discovery/mod.rs`, `src-tauri/src/source/mod.rs`, `src-tauri/src/analysis/mod.rs`, `src-tauri/src/localization/mod.rs`, `src-tauri/src/search/mod.rs`, `src-tauri/src/revisions/mod.rs`, `src-tauri/src/assets/mod.rs`, `src-tauri/src/state/mod.rs`, `src-tauri/src/companion/mod.rs`, `src-tauri/src/canonical/mod.rs`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: the module tree later tasks add files into; `stellaris_docs_lib::run()` re-exported from `composition`; all dependencies and the `test-support` feature declared once so no later task edits `Cargo.toml`.

- [x] **Step 1: Pin the toolchain**

`rust-toolchain.toml` at the repository root:

```toml
[toolchain]
channel = "1.97.1"
components = ["rustfmt", "clippy"]
```

- [x] **Step 2: Replace `src-tauri/Cargo.toml`**

```toml
[package]
name = "stellaris-docs"
version = "0.1.0"
description = "Generates accurate player documentation from installed Stellaris mods"
authors = ["Jackson Yeager"]
edition = "2024"

[lib]
# The `_lib` suffix avoids a Windows bin-name conflict (cargo #8519).
name = "stellaris_docs_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-opener = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
unicode-normalization = "0.1"
num-bigint = "0.4"
num-rational = "0.4"
num-traits = "0.2"
uuid = { version = "1", features = ["v4"] }
tempfile = { version = "3", optional = true }

[features]
# Test-only helpers (isolated app-data directories, later fixture-corpus support).
# Enabled for this package's own tests through the self dev-dependency below;
# never enabled by a production build.
test-support = ["dep:tempfile"]

[dev-dependencies]
stellaris-docs = { path = ".", features = ["test-support"] }
proptest = "1"

[lints.rust]
unsafe_code = "forbid"

[profile.release]
codegen-units = 1
lto = true
opt-level = 3
panic = "abort"
strip = true
```

- [x] **Step 3: Replace `src-tauri/src/lib.rs`**

```rust
//! Application library. The executable target only calls [`run`]; everything else is
//! constructed by the composition root (docs/technical-design.md, "Rust package and
//! dependency direction"). Dependency direction: transports -> application -> deep
//! modules -> filesystem, parser, image-decoder, and persistence adapters.

pub mod analysis;
pub mod application;
pub mod assets;
pub mod canonical;
pub mod companion;
pub mod composition;
pub mod discovery;
pub mod localization;
pub mod revisions;
pub mod search;
pub mod source;
pub mod state;
pub mod transport;

pub use composition::run;
```

- [x] **Step 4: Create the composition root**

`src-tauri/src/composition/mod.rs`:

```rust
//! Composition root: constructs the concrete modules, process-lifetime shared state,
//! background execution resources, and the Tauri application. Framework types stay here
//! and in `transport`; application modules never import Tauri.

/// Scaffold command retained so the scaffold React page keeps working. The Phase 3
/// frontend bootstrap deletes it together with that page.
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! You've been greeted from Rust!")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

`src-tauri/src/main.rs` already only calls `stellaris_docs_lib::run()`; leave it unchanged.

- [x] **Step 5: Create the skeleton modules**

Each file contains only its ownership doc comment. These encode the accepted module map so every later phase has its slot; they are not speculative interfaces.

`src-tauri/src/transport/mod.rs`:

```rust
//! Input adapters: Tauri commands and Companion HTTP requests adapted into shared
//! application DTOs and failure semantics. Adapters call application use cases; they do
//! not implement parallel rules and do not call through one another.

pub mod http;
pub mod tauri;
```

`src-tauri/src/transport/tauri.rs`:

```rust
//! Tauri command adapter. Populated from Phase 3. Inside this module, paths beginning
//! `tauri::` in `use` statements resolve to the Tauri crate, not this module.
```

`src-tauri/src/transport/http.rs`:

```rust
//! Companion HTTP adapter. Populated in Phase 11.
```

`src-tauri/src/application/mod.rs`:

```rust
//! Named product use cases and coordination that genuinely spans deep modules:
//! documentation reads and the Ensure, Rebuild, and Validate workflows
//! (docs/technical-design.md, "Documentation build use cases"). Populated from Phase 3.
```

`src-tauri/src/discovery/mod.rs`:

```rust
//! Finds Stellaris and Mod Installations and reads only the metadata needed to populate
//! the Mod Library. Never a second fingerprint implementation
//! (docs/technical-design.md, "Source module"). Populated in Phase 1.
```

`src-tauri/src/source/mod.rs`:

```rust
//! Sole owner of complete Mod Source traversal and content identity: deterministic
//! enumeration, logical-path normalization and escape rejection, hashing, fingerprints,
//! build-lifetime Source Snapshots, and final live-source verification
//! (docs/technical-design.md, "Source module"). Populated in Phase 2.
```

`src-tauri/src/analysis/mod.rs`:

```rust
//! The deep module that turns Source Snapshots plus typed asset-materialization outcomes
//! into a finalized Revision Candidate. Parser adaptation, content-type-specific
//! resolution, documentation generation, and Source Excerpt capture remain internal
//! submodules (docs/technical-design.md, "Analysis module"). Populated from Phase 4.
```

`src-tauri/src/localization/mod.rs`:

```rust
//! Owns the Stellaris localization language: ingestion, markup tokenization, fallback,
//! Static Localization Reference resolution, plain-text projection, and display tokens
//! (docs/technical-design.md, "Localization module"). Populated in Phase 5.
```

`src-tauri/src/search/mod.rs`:

```rust
//! Owns both sides of the persisted search contract: deterministic index construction,
//! the versioned index representation, and query normalization, matching, ranking, and
//! bounds (docs/technical-design.md, "Host-owned search"). Populated in Phase 7.
```

`src-tauri/src/revisions/mod.rs`:

```rust
//! Sole owner of Documentation Revision bundle I/O: staging, validation, atomic
//! publication, the Revision Reader, handle pinning, and retention
//! (docs/technical-design.md, "Documentation revision publication"). Populated in Phase 3.
```

`src-tauri/src/assets/mod.rs`:

```rust
//! Byte-conversion mechanics behind the shared content-addressed Asset Store: DDS
//! classification, the pinned conversion recipe, typed materialization outcomes, and
//! blob publication (docs/technical-design.md, "Shared content-addressed Asset Store").
//! Populated in Phase 8.
```

`src-tauri/src/state/mod.rs`:

```rust
//! Deep owner of the durable mutable state document: schema, atomic replacement,
//! quarantine recovery, and the narrow publication-reference capability
//! (docs/technical-design.md, "Mutable state storage"). Populated in Phase 1.
```

`src-tauri/src/companion/mod.rs`:

```rust
//! Pairing secrets, Companion Sessions, listener lifecycle, Companion-Ready access
//! policy, and trusted companion revision handles (docs/technical-design.md, "Companion
//! pairing and sessions"). Populated in Phase 11.
```

`src-tauri/src/canonical/mod.rs`:

```rust
//! Shared canonical primitives used by every stable identity: domain-separated digests
//! over a tagged length-prefixed encoding, logical relative paths, and exact numerics
//! (docs/technical-design.md, "Canonicalization and numeric representation").
//!
//! Not part of the technical design's named module map: this is a leaf primitive module
//! below the deep-module row. Each identity's field order and schema remain owned by the
//! module that defines that identity; only the encoding mechanics are shared.
```

- [x] **Step 6: Verify the skeleton compiles clean**

Run (from `src-tauri/`): `cargo fmt && cargo clippy --all-targets --features test-support -- -D warnings && cargo test --features test-support`
Expected: clippy clean; `running 0 tests` … `test result: ok`.

- [x] **Step 7: Commit**

```bash
git add rust-toolchain.toml src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src
git commit -m "Phase 0: pin the toolchain and land the module skeleton"
```

---

### Task 2: Error conventions

**Files:**
- Create: `src-tauri/src/error.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod error;` after `pub mod discovery;`)

**Interfaces:**
- Consumes: `uuid` (v4).
- Produces: `CorrelationId` (`generate() -> Self`, `Display` as 32 hex chars), `Unexpected` (`new(impl Into<String>) -> Self`, `correlation() -> CorrelationId`, `log_detail() -> String`, redacted `Display`), `Failure<E>` (`Expected(E) | Unexpected(Unexpected)`, `From<Unexpected>`), `OpResult<T, E> = Result<T, Failure<E>>`. Phase 1's state module is the first real consumer.

- [x] **Step 1: Write the failing tests**

Create `src-tauri/src/error.rs` containing only the module doc comment and the test module (implementation comes in Step 3), and add `pub mod error;` to `lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlation_ids_are_32_hex_characters_and_unique() {
        let first = CorrelationId::generate();
        let second = CorrelationId::generate();
        let rendered = first.to_string();
        assert_eq!(rendered.len(), 32);
        assert!(rendered.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }

    #[test]
    fn unexpected_display_redacts_detail_but_carries_the_correlation_id() {
        let failure = Unexpected::new("state file vanished mid-mutation: /Users/x/secret");
        let shown = failure.to_string();
        assert!(shown.contains(&failure.correlation().to_string()));
        assert!(!shown.contains("secret"));
        assert!(failure.log_detail().contains("secret"));
    }

    #[test]
    fn unexpected_converts_into_any_failure_union() {
        #[derive(Debug)]
        enum SomeExpected {}
        let failure: Failure<SomeExpected> = Unexpected::new("boom").into();
        assert!(matches!(failure, Failure::Unexpected(_)));
    }
}
```

- [x] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support error::`
Expected: compile error — `CorrelationId` not found.

- [x] **Step 3: Implement**

Prepend to `src-tauri/src/error.rs` above the test module:

```rust
//! Error conventions shared by every module.
//!
//! Expected outcomes cross module and transport boundaries as typed `Result<T, E>` where
//! `E` is an operation-specific union (docs/technical-design.md, "Serializable result
//! contract"). Unexpected failures — invariant violations, corrupted cross-module
//! contracts, programmer defects — travel as [`Unexpected`], carrying a correlation
//! identifier that transports may show while detailed chains stay in protected desktop
//! logs.
//!
//! Panic policy: a panic is a defect, never control flow. Nothing intentionally panics
//! across a module boundary, and no panic is serialized to a transport. Transport
//! entrypoints catch and redact unexpected failures where the runtime can safely unwind.

use std::fmt;

/// Opaque identifier correlating a user-visible failure with protected log detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CorrelationId([u8; 16]);

impl CorrelationId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().into_bytes())
    }
}

impl fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// An unexpected internal failure: not a member of any operation's expected-error union.
#[derive(Debug)]
pub struct Unexpected {
    correlation: CorrelationId,
    message: String,
}

impl Unexpected {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            correlation: CorrelationId::generate(),
            message: message.into(),
        }
    }

    pub fn correlation(&self) -> CorrelationId {
        self.correlation
    }

    /// Full detail for protected desktop logs only. Never crosses a transport.
    pub fn log_detail(&self) -> String {
        format!("[{}] {}", self.correlation, self.message)
    }
}

impl fmt::Display for Unexpected {
    /// Redacted rendering, safe for transports: correlation identifier only.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unexpected internal error [{}]", self.correlation)
    }
}

impl std::error::Error for Unexpected {}

/// The failure channel of an application operation: an expected, typed refusal or an
/// unexpected internal error.
#[derive(Debug)]
pub enum Failure<E> {
    Expected(E),
    Unexpected(Unexpected),
}

impl<E> From<Unexpected> for Failure<E> {
    fn from(unexpected: Unexpected) -> Self {
        Self::Unexpected(unexpected)
    }
}

pub type OpResult<T, E> = Result<T, Failure<E>>;
```

- [x] **Step 4: Run and watch it pass**

Run: `cargo test --features test-support error::`
Expected: 3 passed.

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/error.rs src-tauri/src/lib.rs
git commit -m "Phase 0: error conventions with correlation identifiers"
```

---

### Task 3: Canonical digest encoder

**Files:**
- Create: `src-tauri/src/canonical/encode.rs`
- Modify: `src-tauri/src/canonical/mod.rs` (add `pub mod encode;` below the doc comment)

**Interfaces:**
- Consumes: `sha2`.
- Produces: `ENCODING_VERSION: u32`; `CanonicalDigest` (`new(domain: &str)`, chainable `&mut Self` methods `bytes(&[u8])`, `text(&str)`, `u64(u64)`, `bool(bool)`, `begin_seq(usize)`, `begin_map(usize)`, `none()`, `some()`, and `finish(self) -> DigestBytes`); `DigestBytes([u8; 32])` with `to_hex() -> String` and `Display`. Tasks 5–6 and every later identity encode through this.

- [x] **Step 1: Write the failing tests**

Create `src-tauri/src/canonical/encode.rs` with only this test module, and register the submodule in `canonical/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn digest_of(build: impl FnOnce(&mut CanonicalDigest)) -> DigestBytes {
        let mut digest = CanonicalDigest::new("stellaris-docs/encode-test/v1");
        build(&mut digest);
        digest.finish()
    }

    #[test]
    fn framing_distinguishes_concatenation_from_separate_items() {
        let joined = digest_of(|d| {
            d.begin_seq(1).text("ab");
        });
        let split = digest_of(|d| {
            d.begin_seq(2).text("a").text("b");
        });
        assert_ne!(joined, split);
    }

    #[test]
    fn framing_distinguishes_types_with_identical_bytes() {
        let text = digest_of(|d| {
            d.text("a");
        });
        let bytes = digest_of(|d| {
            d.bytes(b"a");
        });
        assert_ne!(text, bytes);
    }

    #[test]
    fn domains_separate_identical_bodies() {
        let mut first = CanonicalDigest::new("stellaris-docs/a/v1");
        first.u64(1);
        let mut second = CanonicalDigest::new("stellaris-docs/b/v1");
        second.u64(1);
        assert_ne!(first.finish(), second.finish());
    }

    #[test]
    fn encoded_none_differs_from_absence() {
        let with_none = digest_of(|d| {
            d.u64(1).none();
        });
        let absent = digest_of(|d| {
            d.u64(1);
        });
        assert_ne!(with_none, absent);
    }

    #[test]
    fn pinned_self_test_digest() {
        // Pinned regression value. Any framing or tag change must consciously bump
        // ENCODING_VERSION (and the analysis version vector's canonical_encoding
        // component), then re-pin this value.
        let mut digest = CanonicalDigest::new("stellaris-docs/canonical-selftest/v1");
        digest
            .text("tech_lasers_1")
            .u64(3)
            .bool(true)
            .begin_seq(2)
            .text("a")
            .text("b")
            .begin_map(1)
            .text("k")
            .u64(7)
            .none()
            .some()
            .u64(9)
            .bytes(&[0x00, 0xff]);
        assert_eq!(
            digest.finish().to_hex(),
            "92545b5ae23bba69580a855a9ac49c9422b60cd45f108e6ae752b4c201216581"
        );
    }
}
```

- [x] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support canonical::encode::`
Expected: compile error — `CanonicalDigest` not found.

- [x] **Step 3: Implement**

Prepend above the test module:

```rust
//! Domain-separated canonical digests over a tagged, length-prefixed encoding.
//!
//! Every stable identity (fingerprints, Revision identifiers, Hidden Route identities,
//! asset keys) hashes this encoding rather than serializer output or map iteration order
//! (docs/technical-design.md, "Canonicalization and numeric representation"). Framing is
//! tagged and length-prefixed so no two distinct values share a byte stream by
//! concatenation — the same rule the parser spike's digest proved out. The encoding is
//! one-way: nothing decodes it, so evolution means a new domain version and a bumped
//! [`ENCODING_VERSION`], never in-place reinterpretation.
//!
//! Composite framing contract (enforced by callers, stated here once):
//! - `begin_seq(len)` is followed by exactly `len` encoded items.
//! - `begin_map(len)` is followed by `len` key–value pairs, keys already in canonical
//!   UTF-8 byte order; iterating a `BTreeMap<String, _>` satisfies this.
//! - `some()` is followed by exactly one encoded value; `none()` stands alone.

use sha2::{Digest, Sha256};
use std::fmt;

/// Encoding format version. Participates in the analysis version vector as its
/// `canonical_encoding` component.
pub const ENCODING_VERSION: u32 = 1;

mod tag {
    pub const BYTES: u8 = 0x01;
    pub const TEXT: u8 = 0x02;
    pub const U64: u8 = 0x03;
    pub const BOOL: u8 = 0x04;
    pub const SEQ: u8 = 0x05;
    pub const MAP: u8 = 0x06;
    pub const NONE: u8 = 0x07;
    pub const SOME: u8 = 0x08;
}

/// A finished 32-byte canonical digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DigestBytes(pub [u8; 32]);

impl DigestBytes {
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Display for DigestBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

pub struct CanonicalDigest {
    hasher: Sha256,
}

impl CanonicalDigest {
    /// `domain` names the identity and its version, e.g. `stellaris-docs/asset-key/v1`.
    pub fn new(domain: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(domain.as_bytes());
        hasher.update([0]);
        Self { hasher }
    }

    pub fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.hasher.update([tag::BYTES]);
        self.len(value.len());
        self.hasher.update(value);
        self
    }

    pub fn text(&mut self, value: &str) -> &mut Self {
        self.hasher.update([tag::TEXT]);
        self.len(value.len());
        self.hasher.update(value.as_bytes());
        self
    }

    pub fn u64(&mut self, value: u64) -> &mut Self {
        self.hasher.update([tag::U64]);
        self.hasher.update(value.to_be_bytes());
        self
    }

    pub fn bool(&mut self, value: bool) -> &mut Self {
        self.hasher.update([tag::BOOL, u8::from(value)]);
        self
    }

    pub fn begin_seq(&mut self, len: usize) -> &mut Self {
        self.hasher.update([tag::SEQ]);
        self.len(len);
        self
    }

    pub fn begin_map(&mut self, len: usize) -> &mut Self {
        self.hasher.update([tag::MAP]);
        self.len(len);
        self
    }

    pub fn none(&mut self) -> &mut Self {
        self.hasher.update([tag::NONE]);
        self
    }

    pub fn some(&mut self) -> &mut Self {
        self.hasher.update([tag::SOME]);
        self
    }

    pub fn finish(self) -> DigestBytes {
        DigestBytes(self.hasher.finalize().into())
    }

    fn len(&mut self, len: usize) {
        self.hasher.update((len as u64).to_be_bytes());
    }
}
```

- [x] **Step 4: Run and watch it pass**

Run: `cargo test --features test-support canonical::encode::`
Expected: 5 passed, including the pinned digest.

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/canonical
git commit -m "Phase 0: canonical domain-separated digest encoder"
```

---

### Task 4: Logical paths

**Files:**
- Create: `src-tauri/src/canonical/path.rs`
- Modify: `src-tauri/src/canonical/mod.rs` (add `pub mod path;`)

**Interfaces:**
- Consumes: `unicode-normalization`.
- Produces: `LogicalPath` (`parse(&str) -> Result<Self, PathError>`, `from_raw_bytes(&[u8]) -> Result<Self, PathError>`, `as_str() -> &str`, `components()`, derived `Ord` by normalized UTF-8 bytes) and `PathError` (`InvalidUnicode | Empty | AbsolutePrefix | EmptyComponent | DotComponent | BackslashComponent | NulByte`). Phases 1–2 build installation identity and enumeration on this.

- [x] **Step 1: Write the failing tests**

Create `src-tauri/src/canonical/path.rs` with only this test module and register it:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use unicode_normalization::is_nfc;

    #[test]
    fn nfd_and_nfc_input_normalize_to_the_same_path() {
        let decomposed = LogicalPath::parse("common/te\u{0301}ch.txt").unwrap();
        let composed = LogicalPath::parse("common/t\u{e9}ch.txt").unwrap();
        assert_eq!(decomposed, composed);
        assert!(is_nfc(decomposed.as_str()));
    }

    #[test]
    fn case_only_variants_remain_distinct() {
        let lower = LogicalPath::parse("common/tech.txt").unwrap();
        let upper = LogicalPath::parse("common/Tech.txt").unwrap();
        assert_ne!(lower, upper);
    }

    #[test]
    fn rejections() {
        assert_eq!(LogicalPath::parse(""), Err(PathError::Empty));
        assert_eq!(LogicalPath::parse("/common/a.txt"), Err(PathError::AbsolutePrefix));
        assert_eq!(LogicalPath::parse("C:/mods/a.txt"), Err(PathError::AbsolutePrefix));
        assert_eq!(LogicalPath::parse("a//b.txt"), Err(PathError::EmptyComponent));
        assert_eq!(LogicalPath::parse("a/b/"), Err(PathError::EmptyComponent));
        assert_eq!(LogicalPath::parse("./a.txt"), Err(PathError::DotComponent));
        assert_eq!(LogicalPath::parse("a/../b.txt"), Err(PathError::DotComponent));
        assert_eq!(LogicalPath::parse("a\\b.txt"), Err(PathError::BackslashComponent));
        assert_eq!(LogicalPath::parse("a\0b"), Err(PathError::NulByte));
        assert_eq!(
            LogicalPath::from_raw_bytes(&[0x66, 0xff, 0x6f]),
            Err(PathError::InvalidUnicode)
        );
    }

    #[test]
    fn colon_is_only_special_as_a_drive_prefix() {
        assert!(LogicalPath::parse("events/a:b.txt").is_ok());
        assert_eq!(LogicalPath::parse("c:"), Err(PathError::AbsolutePrefix));
    }

    const PATH_RE: &str = "[a-zA-Z0-9._-]{1,12}(/[a-zA-Z0-9._-]{1,12}){0,4}";

    fn no_dot_components(raw: &str) -> bool {
        raw.split('/').all(|component| component != "." && component != "..")
    }

    proptest! {
        #[test]
        fn parse_is_idempotent(raw in PATH_RE) {
            prop_assume!(no_dot_components(&raw));
            let first = LogicalPath::parse(&raw).unwrap();
            let second = LogicalPath::parse(first.as_str()).unwrap();
            prop_assert_eq!(first, second);
        }

        #[test]
        fn ordering_matches_normalized_utf8_bytes(a in PATH_RE, b in PATH_RE) {
            prop_assume!(no_dot_components(&a) && no_dot_components(&b));
            let left = LogicalPath::parse(&a).unwrap();
            let right = LogicalPath::parse(&b).unwrap();
            let by_bytes = left.as_str().as_bytes().cmp(right.as_str().as_bytes());
            prop_assert_eq!(left.cmp(&right), by_bytes);
        }
    }
}
```

- [x] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support canonical::path::`
Expected: compile error — `LogicalPath` not found.

- [x] **Step 3: Implement**

Prepend above the test module:

```rust
//! Logical relative paths: the identity form for every file inside a Discovery Location
//! or Mod Source tree (docs/technical-design.md, "Installation identity").
//!
//! `/` separators, Unicode NFC, exact case-preserving comparison, no `.` or `..`
//! components, ordering by normalized UTF-8 bytes. Windows drive letters and root
//! prefixes never enter. Invalid Unicode is rejected without lossy conversion. Collision
//! visibility — two distinct raw entries normalizing to one logical path — is the
//! enumerating caller's job (Phase 2); this type makes it possible by being
//! deterministic.

use std::fmt;
use unicode_normalization::UnicodeNormalization;

/// An NFC-normalized, `/`-separated relative path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogicalPath(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    InvalidUnicode,
    Empty,
    AbsolutePrefix,
    /// Leading, trailing, or doubled separators.
    EmptyComponent,
    /// `.` or `..`.
    DotComponent,
    BackslashComponent,
    NulByte,
}

impl LogicalPath {
    /// Raw bytes from filesystem enumeration. Non-UTF-8 is rejected, never lossily
    /// converted.
    pub fn from_raw_bytes(raw: &[u8]) -> Result<Self, PathError> {
        let text = std::str::from_utf8(raw).map_err(|_| PathError::InvalidUnicode)?;
        Self::parse(text)
    }

    pub fn parse(raw: &str) -> Result<Self, PathError> {
        if raw.is_empty() {
            return Err(PathError::Empty);
        }
        if raw.contains('\0') {
            return Err(PathError::NulByte);
        }
        if raw.contains('\\') {
            return Err(PathError::BackslashComponent);
        }
        if raw.starts_with('/') || has_drive_prefix(raw) {
            return Err(PathError::AbsolutePrefix);
        }
        for component in raw.split('/') {
            if component.is_empty() {
                return Err(PathError::EmptyComponent);
            }
            if component == "." || component == ".." {
                return Err(PathError::DotComponent);
            }
        }
        Ok(Self(raw.nfc().collect()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

impl fmt::Display for LogicalPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn has_drive_prefix(raw: &str) -> bool {
    let mut chars = raw.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic()
    )
}
```

- [x] **Step 4: Run and watch it pass**

Run: `cargo test --features test-support canonical::path::`
Expected: 4 unit tests + 2 property tests pass.

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/canonical
git commit -m "Phase 0: logical path normalization and ordering"
```

---

### Task 5: Exact numeric representation

**Files:**
- Create: `src-tauri/src/canonical/numeric.rs`
- Modify: `src-tauri/src/canonical/mod.rs` (add `pub mod numeric;`)

**Interfaces:**
- Consumes: `CanonicalDigest` from Task 3; `num-bigint`, `num-rational`, `num-traits`.
- Produces: `SourceNumber` (`parse(&str) -> Self`, `lexeme() -> &str`, `value() -> Option<&ExactValue>`) and `ExactValue` (`add/sub/mul(&Self) -> Self`, `div(&Self) -> Option<Self>`, `encode(&mut CanonicalDigest)`, `to_decimal_string() -> Option<String>`, `Eq + Ord + Hash`). Phase 4's parsed model wraps numeric scalars in `SourceNumber`; Phase 6's Resolved Base Values use `ExactValue` arithmetic.

- [x] **Step 1: Write the failing tests**

Create `src-tauri/src/canonical/numeric.rs` with only this test module and register it:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn value_of(lexeme: &str) -> ExactValue {
        SourceNumber::parse(lexeme).value().cloned().unwrap()
    }

    #[test]
    fn preserves_the_lexeme_verbatim() {
        let number = SourceNumber::parse("007.500");
        assert_eq!(number.lexeme(), "007.500");
        assert_eq!(number.value(), SourceNumber::parse("7.5").value());
    }

    #[test]
    fn one_tenth_plus_two_tenths_is_exactly_three_tenths() {
        let sum = value_of("0.1").add(&value_of("0.2"));
        assert_eq!(sum, value_of("0.3"));
    }

    #[test]
    fn unsupported_lexemes_stay_symbolic_with_lexeme_preserved() {
        for lexeme in ["1e5", "5.", "1.2.3", "0x10", "--1", "+", "@base_cost"] {
            let number = SourceNumber::parse(lexeme);
            assert_eq!(number.lexeme(), lexeme, "lexeme {lexeme}");
            assert!(number.value().is_none(), "lexeme {lexeme}");
        }
    }

    #[test]
    fn division_by_zero_is_unresolved_not_a_panic_or_approximation() {
        assert!(value_of("1").div(&value_of("0")).is_none());
    }

    #[test]
    fn decimal_rendering_terminates_or_declines() {
        assert_eq!(value_of("2.50").to_decimal_string().as_deref(), Some("2.5"));
        assert_eq!(value_of("-0.125").to_decimal_string().as_deref(), Some("-0.125"));
        assert_eq!(value_of("40").to_decimal_string().as_deref(), Some("40"));
        let third = value_of("1").div(&value_of("3")).unwrap();
        assert_eq!(third.to_decimal_string(), None);
    }

    #[test]
    fn encoding_distinguishes_close_values() {
        use crate::canonical::encode::CanonicalDigest;
        let digest_for = |value: &ExactValue| {
            let mut digest = CanonicalDigest::new("stellaris-docs/numeric-test/v1");
            value.encode(&mut digest);
            digest.finish()
        };
        assert_ne!(digest_for(&value_of("0.1")), digest_for(&value_of("0.10001")));
        assert_eq!(digest_for(&value_of("0.10")), digest_for(&value_of("0.1")));
    }

    proptest! {
        #[test]
        fn integer_arithmetic_matches_i64(
            a in -1_000_000i64..1_000_000,
            b in -1_000_000i64..1_000_000,
        ) {
            let left = value_of(&a.to_string());
            let right = value_of(&b.to_string());
            prop_assert_eq!(left.add(&right), value_of(&(a + b).to_string()));
            prop_assert_eq!(left.sub(&right), value_of(&(a - b).to_string()));
            prop_assert_eq!(left.mul(&right), value_of(&(a * b).to_string()));
        }

        #[test]
        fn decimal_rendering_round_trips(
            int in 0u32..100_000,
            frac in 0u32..10_000,
        ) {
            let lexeme = format!("{int}.{frac:04}");
            let value = value_of(&lexeme);
            let rendered = value.to_decimal_string().unwrap();
            prop_assert_eq!(&value_of(&rendered), &value);
        }

        #[test]
        fn multiply_then_divide_round_trips(
            a in -1_000_000i64..1_000_000,
            b in 1i64..1_000_000,
        ) {
            let left = value_of(&a.to_string());
            let right = value_of(&b.to_string());
            let round_tripped = left.mul(&right).div(&right).unwrap();
            prop_assert_eq!(round_tripped, left);
        }
    }
}
```

- [x] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support canonical::numeric::`
Expected: compile error — `SourceNumber` not found.

- [x] **Step 3: Implement**

Prepend above the test module:

```rust
//! Exact numeric representation for source values (docs/technical-design.md,
//! "Canonicalization and numeric representation").
//!
//! A parsed number preserves its original lexeme and, when the lexeme is an integer or
//! finite base-10 decimal, an exact rational value. Deterministic static arithmetic
//! operates on that exact form. Binary floating point never participates in equality,
//! hashing, identity, or displayed exact values. An operation without a proven exact
//! result — division by zero here; unproven Stellaris rounding semantics later — yields
//! `None` and stays visibly unresolved, never approximated.

use crate::canonical::encode::CanonicalDigest;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, Zero};

/// A numeric scalar as it appeared in source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceNumber {
    lexeme: String,
    value: Option<ExactValue>,
}

impl SourceNumber {
    /// The lexeme is always preserved verbatim; a value is present only when the lexeme
    /// is a supported exact form.
    pub fn parse(lexeme: &str) -> Self {
        Self {
            lexeme: lexeme.to_owned(),
            value: parse_exact(lexeme).map(ExactValue),
        }
    }

    pub fn lexeme(&self) -> &str {
        &self.lexeme
    }

    pub fn value(&self) -> Option<&ExactValue> {
        self.value.as_ref()
    }
}

/// Supported forms: `[+-]?digits`, `[+-]?digits.digits`, `[+-]?.digits`.
/// A trailing dot (`5.`) is unproven in source and stays symbolic.
fn parse_exact(lexeme: &str) -> Option<BigRational> {
    let unsigned = lexeme.strip_prefix(['+', '-']).unwrap_or(lexeme);
    let (integer, fraction) = match unsigned.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (unsigned, ""),
    };
    if integer.is_empty() && fraction.is_empty() {
        return None;
    }
    if unsigned.contains('.') && fraction.is_empty() {
        return None;
    }
    if !integer.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{integer}{fraction}");
    let numerator: BigInt = digits.parse().ok()?;
    let denominator = num_traits::pow(BigInt::from(10), fraction.len());
    let magnitude = BigRational::new(numerator, denominator);
    Some(if lexeme.starts_with('-') { -magnitude } else { magnitude })
}

/// An exact rational produced by parsing or deterministic static arithmetic.
/// `BigRational` keeps a reduced canonical form, so `Eq`, `Ord`, and `Hash` agree on
/// mathematically equal values.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExactValue(BigRational);

impl ExactValue {
    pub fn add(&self, other: &Self) -> Self {
        Self(&self.0 + &other.0)
    }

    pub fn sub(&self, other: &Self) -> Self {
        Self(&self.0 - &other.0)
    }

    pub fn mul(&self, other: &Self) -> Self {
        Self(&self.0 * &other.0)
    }

    /// `None` when `other` is zero: unresolved, never an approximation or panic.
    pub fn div(&self, other: &Self) -> Option<Self> {
        if other.0.is_zero() {
            None
        } else {
            Some(Self(&self.0 / &other.0))
        }
    }

    /// Canonical identity contribution: sign, numerator magnitude, denominator, all in
    /// reduced form, as decimal digit strings.
    pub fn encode(&self, digest: &mut CanonicalDigest) {
        digest
            .bool(self.0.is_negative())
            .text(&self.0.numer().magnitude().to_string())
            .text(&self.0.denom().to_string());
    }

    /// Decimal rendering when the reduced denominator is `2^a * 5^b`; `None` otherwise.
    /// Trailing fraction zeros are trimmed for display; identity uses [`Self::encode`].
    pub fn to_decimal_string(&self) -> Option<String> {
        let two = BigInt::from(2);
        let five = BigInt::from(5);
        let mut reduced = self.0.denom().clone();
        let mut twos = 0usize;
        let mut fives = 0usize;
        while (&reduced % &two).is_zero() {
            reduced = &reduced / &two;
            twos += 1;
        }
        while (&reduced % &five).is_zero() {
            reduced = &reduced / &five;
            fives += 1;
        }
        if reduced != BigInt::from(1) {
            return None;
        }
        let scale = twos.max(fives);
        let scaled = (self.0.numer() * num_traits::pow(BigInt::from(10), scale))
            / self.0.denom();
        let digits = scaled.magnitude().to_string();
        let sign = if self.0.is_negative() { "-" } else { "" };
        if scale == 0 {
            return Some(format!("{sign}{digits}"));
        }
        let padded = format!("{digits:0>width$}", width = scale + 1);
        let (integer, fraction) = padded.split_at(padded.len() - scale);
        let fraction = fraction.trim_end_matches('0');
        Some(if fraction.is_empty() {
            format!("{sign}{integer}")
        } else {
            format!("{sign}{integer}.{fraction}")
        })
    }
}
```

- [x] **Step 4: Run and watch it pass**

Run: `cargo test --features test-support canonical::numeric::`
Expected: 6 unit tests + 3 property tests pass.

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/canonical
git commit -m "Phase 0: exact numeric representation"
```

---

### Task 6: Analysis version vector

**Files:**
- Create: `src-tauri/src/analysis/version.rs`
- Modify: `src-tauri/src/analysis/mod.rs` (add `pub mod version;` below the doc comment)

**Interfaces:**
- Consumes: `CanonicalDigest`, `DigestBytes`, `ENCODING_VERSION` from Task 3.
- Produces: `AnalysisVersionVector` with public `u32` fields `source_enumeration`, `parsed_model`, `resolution_profile`, `documentation`, `localization_interpretation`, `search`, `canonical_encoding`, `hidden_route_identity`, `analysis_issue_propagation`, plus `asset_recipes: BTreeMap<String, u32>`; `current() -> Self`; `digest(&self) -> DigestBytes`. Phase 3's manifest embeds it; later phases bump their component when semantics change.

- [x] **Step 1: Write the failing tests**

Create `src-tauri/src/analysis/version.rs` with only this test module and register it:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::encode::ENCODING_VERSION;

    #[test]
    fn current_tracks_the_canonical_encoding_version() {
        assert_eq!(AnalysisVersionVector::current().canonical_encoding, ENCODING_VERSION);
    }

    #[test]
    fn pinned_current_digest() {
        // Pinned regression value: any component bump changes this and must re-pin it,
        // which is exactly the review moment the version vector exists to force.
        assert_eq!(
            AnalysisVersionVector::current().digest().to_hex(),
            "49cfc070bfea607cf7b100761587ebe9599e0ce106e963736ae3882506c98641"
        );
    }

    #[test]
    fn every_component_and_recipe_participates_in_the_digest() {
        let base = AnalysisVersionVector::current();
        let mut bumped = base.clone();
        bumped.parsed_model += 1;
        assert_ne!(base.digest(), bumped.digest());

        let mut with_recipe = base.clone();
        with_recipe.asset_recipes.insert("dds-to-png".to_owned(), 1);
        assert_ne!(base.digest(), with_recipe.digest());
    }
}
```

- [x] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support analysis::version::`
Expected: compile error — `AnalysisVersionVector` not found.

- [x] **Step 3: Implement**

Prepend above the test module:

```rust
//! The analysis version vector: every semantic component whose change invalidates
//! previously built revisions (docs/technical-design.md, "Canonicalization and numeric
//! representation"). Each component starts at 1 and is bumped by hand in the phase that
//! changes the component's behavior.

use crate::canonical::encode::{CanonicalDigest, DigestBytes, ENCODING_VERSION};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisVersionVector {
    pub source_enumeration: u32,
    pub parsed_model: u32,
    pub resolution_profile: u32,
    /// Documentation schema and generator together.
    pub documentation: u32,
    pub localization_interpretation: u32,
    /// Search normalization and index schema together.
    pub search: u32,
    pub canonical_encoding: u32,
    pub hidden_route_identity: u32,
    pub analysis_issue_propagation: u32,
    /// One entry per asset conversion recipe, keyed by recipe identifier.
    /// Empty until Phase 8 registers the DDS-to-PNG recipe.
    pub asset_recipes: BTreeMap<String, u32>,
}

impl AnalysisVersionVector {
    pub fn current() -> Self {
        Self {
            source_enumeration: 1,
            parsed_model: 1,
            resolution_profile: 1,
            documentation: 1,
            localization_interpretation: 1,
            search: 1,
            canonical_encoding: ENCODING_VERSION,
            hidden_route_identity: 1,
            analysis_issue_propagation: 1,
            asset_recipes: BTreeMap::new(),
        }
    }

    pub fn digest(&self) -> DigestBytes {
        let mut digest = CanonicalDigest::new("stellaris-docs/analysis-version-vector/v1");
        digest
            .u64(self.source_enumeration.into())
            .u64(self.parsed_model.into())
            .u64(self.resolution_profile.into())
            .u64(self.documentation.into())
            .u64(self.localization_interpretation.into())
            .u64(self.search.into())
            .u64(self.canonical_encoding.into())
            .u64(self.hidden_route_identity.into())
            .u64(self.analysis_issue_propagation.into())
            .begin_map(self.asset_recipes.len());
        for (recipe, version) in &self.asset_recipes {
            digest.text(recipe).u64((*version).into());
        }
        digest.finish()
    }
}
```

- [x] **Step 4: Run and watch it pass**

Run: `cargo test --features test-support analysis::version::`
Expected: 3 passed, including the pinned digest.

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/analysis
git commit -m "Phase 0: analysis version vector"
```

---

### Task 7: Test-support feature and isolated application data

**Files:**
- Create: `src-tauri/src/testsupport/mod.rs`
- Modify: `src-tauri/src/lib.rs` (add the gated module declaration)
- Test: `src-tauri/tests/test_support.rs`

**Interfaces:**
- Consumes: `tempfile` (optional dependency wired in Task 1).
- Produces: `testsupport::TempAppData` (`new() -> Self`, `path() -> &Path`), compiled only under the `test-support` feature. Phase 1 state tests and the Phase 3 acceptance harness construct isolated application-data directories through this.

- [x] **Step 1: Write the failing integration test**

`src-tauri/tests/test_support.rs`:

```rust
use std::fs;
use stellaris_docs_lib::testsupport::TempAppData;

#[test]
fn each_instance_is_an_isolated_writable_directory() {
    let first = TempAppData::new();
    let second = TempAppData::new();
    assert_ne!(first.path(), second.path());
    fs::write(first.path().join("state.json"), b"{}").unwrap();
    assert!(!second.path().join("state.json").exists());
}

#[test]
fn the_directory_is_removed_on_drop() {
    let path = {
        let data = TempAppData::new();
        assert!(data.path().is_dir());
        data.path().to_path_buf()
    };
    assert!(!path.exists());
}
```

- [x] **Step 2: Run and watch it fail**

Run: `cargo test --features test-support --test test_support`
Expected: compile error — no `testsupport` module.

- [x] **Step 3: Implement**

Add to `src-tauri/src/lib.rs` after the `pub mod transport;` line:

```rust
#[cfg(feature = "test-support")]
pub mod testsupport;
```

`src-tauri/src/testsupport/mod.rs`:

```rust
//! Test-only helpers, compiled solely under the `test-support` feature. Production
//! builds never enable the feature. Phase 2 adds source-owned fixture-corpus support
//! here alongside the source module's own test seam.

use std::path::Path;
use tempfile::TempDir;

/// An isolated, disposable application-data directory for one test.
///
/// Every constructed value owns a distinct directory that disappears on drop. This is
/// the caller-precondition isolation the single-instance design demands of tests and
/// development tooling (docs/technical-design.md, "Single-instance ownership").
pub struct TempAppData {
    root: TempDir,
}

impl TempAppData {
    pub fn new() -> Self {
        Self {
            root: TempDir::new().expect("create temporary application-data directory"),
        }
    }

    pub fn path(&self) -> &Path {
        self.root.path()
    }
}

impl Default for TempAppData {
    fn default() -> Self {
        Self::new()
    }
}
```

- [x] **Step 4: Run and watch it pass**

Run: `cargo test --features test-support --test test_support`
Expected: 2 passed.

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/testsupport src-tauri/src/lib.rs src-tauri/tests/test_support.rs
git commit -m "Phase 0: test-support feature and isolated app-data helper"
```

---

### Task 8: Frontend test harness

**Files:**
- Modify: `package.json` (add `test` script; `vitest` devDependency via install)
- Create: `src/harness.test.ts`

**Interfaces:**
- Consumes: the existing Vite + TypeScript scaffold.
- Produces: `npm test` running Vitest. Phase 3's documentation-client and Result-decoding tests replace the placeholder.

- [x] **Step 1: Install Vitest and add the script**

Run: `npm install --save-dev vitest`
Then add to `package.json` `scripts`:

```json
"test": "vitest run"
```

- [x] **Step 2: Write a failing harness test**

`src/harness.test.ts`:

```ts
import { describe, expect, it } from "vitest";

// Proves the frontend test gate runs and can fail. The first real TypeScript module
// tests (Phase 3 documentation client) replace this file.
describe("frontend test harness", () => {
  it("runs", () => {
    expect(true).toBe(false);
  });
});
```

Run: `npm test`
Expected: 1 failed — the harness detects failure.

- [x] **Step 3: Make it pass**

Change the assertion to `expect(true).toBe(true);`.

Run: `npm test`
Expected: 1 passed.

- [x] **Step 4: Verify the type-check build still passes**

Run: `npm run build`
Expected: `tsc` and `vite build` succeed (Vitest types resolve through the package import).

- [x] **Step 5: Commit**

```bash
git add package.json package-lock.json src/harness.test.ts
git commit -m "Phase 0: frontend test harness"
```

---

### Task 9: CI gate and negative controls

**Files:**
- Create: `tools/ci/check.sh` (mode 755)
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: every gate from Tasks 1–8.
- Produces: one local command (`tools/ci/check.sh`) and one workflow running the identical gates. Every later phase's definition of done includes this script exiting 0.

- [x] **Step 1: Write the local gate script**

`tools/ci/check.sh`:

```bash
#!/usr/bin/env bash
# Every gate CI runs, in the same order. A change is complete only when this exits 0
# (AGENTS.md: complete the feedback loop).
set -euo pipefail
cd "$(dirname "$0")/../.."

(cd src-tauri && cargo fmt --check)
(cd src-tauri && cargo clippy --all-targets --features test-support -- -D warnings)
(cd src-tauri && cargo test --features test-support)
npm run build
npm test
```

Run: `chmod +x tools/ci/check.sh && tools/ci/check.sh`
Expected: exits 0 with all gates green.

- [x] **Step 2: Negative control — Rust gate can fail**

Create a scratch file `src-tauri/tests/negative_control.rs`:

```rust
#[test]
fn negative_control() {
    let observed: Vec<u8> = Vec::new();
    assert_eq!(
        observed.len(),
        1,
        "deliberate failure proving the gate detects red"
    );
}
```

Run: `tools/ci/check.sh`
Expected: nonzero exit at the `cargo test` step, naming `negative_control`.

As executed, this snippet replaces the plan's original `assert!(false, ...)`. That form is
constant-folded by `clippy::assertions_on_constants`, so the gate went red at the clippy
step and never reached `cargo test` — proving the lint gate rather than the test gate. The
runtime comparison above reaches the test runner, which is what this step exists to prove.
Observed: exit 101 at `cargo test`, `test negative_control ... FAILED`; exit 0 after
removal.

Then: `rm src-tauri/tests/negative_control.rs` and rerun `tools/ci/check.sh`.
Expected: exits 0.

- [x] **Step 3: Negative control — frontend gate can fail**

Temporarily change `src/harness.test.ts` back to `expect(true).toBe(false)`, run `tools/ci/check.sh`, confirm nonzero exit at `npm test`, revert, rerun, confirm green.

- [x] **Step 4: Write the workflow**

`.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  rust:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: "1.97.1"
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
      - name: Format
        run: cargo fmt --check
        working-directory: src-tauri
      - name: Clippy
        run: cargo clippy --all-targets --features test-support -- -D warnings
        working-directory: src-tauri
      - name: Test
        run: cargo test --features test-support
        working-directory: src-tauri

  frontend:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 24
          cache: npm
      - run: npm ci
      - run: npm run build
      - run: npm test
```

- [x] **Step 5: Commit**

```bash
git add tools/ci/check.sh .github/workflows/ci.yml
git commit -m "Phase 0: CI gate with negative-control verification"
```

The workflow itself is verified on the next push to `origin` — pushing is left to the user. If the first Actions run fails for environmental reasons (runner image, action versions), fix forward; the local script remains the authoritative gate either way.

---

### Task 10: Record foundation decisions

**Files:**
- Modify: `docs/decision-log.md` (append an entry; match the file's existing entry format)
- Modify: `docs/implementation-plan.md` (mark Phase 0 as implemented, once Tasks 1–9 are done)

**Interfaces:**
- Consumes: the completed tasks above.
- Produces: a durable record of the three deviations flagged in the header.

- [x] **Step 1: Append a decision-log entry**

Record, in the log's existing format, dated the day of completion:

- `canonical` and `error` added as top-level leaf modules beneath the technical design's module map; each identity's field order and schema remain owned by its defining module, only encoding mechanics and error conventions are shared.
- Rust edition bumped to 2024 while the crate was empty; toolchain pinned to 1.97.1 via `rust-toolchain.toml` for reproducible builds and CI.
- The `test-support` cargo feature (self dev-dependency pattern) is the standing mechanism for product test seams; production builds never enable it.

- [x] **Step 2: Update the outline**

In `docs/implementation-plan.md`, change the Phase 0 heading to note completion and link this plan: `## Phase 0 — Foundations (implemented — [detailed plan](./plans/phase-0-foundations.md))`.

- [x] **Step 3: Run the full gate one final time**

Run: `tools/ci/check.sh`
Expected: exits 0.

- [x] **Step 4: Commit**

```bash
git add docs/decision-log.md docs/implementation-plan.md
git commit -m "Phase 0: record foundation decisions"
```

---

## Self-review

- **Outline coverage:** outline item 1 (layout, composition root, executable) → Task 1; item 2 (error conventions) → Task 2; item 3 (canonical encoding) → Tasks 3–4; item 4 (exact numerics) → Task 5; item 5 (version vector) → Task 6; item 6 (test infra + CI) → Tasks 7–9. Exit criteria: compiles on macOS CI (Task 9), canonicalization and numeric property tests (Tasks 4–5), negative control proving the gate fails (Task 9 Steps 2–3).
- **Placeholders:** none — every step carries complete code or an exact command. The two pinned digests are precomputed real values, not stand-ins.
- **Type consistency:** `CanonicalDigest`/`DigestBytes`/`ENCODING_VERSION` names and signatures match across Tasks 3, 5, and 6; `ExactValue::encode(&mut CanonicalDigest)` uses only methods Task 3 defines (`bool`, `text`); Task 6 uses `begin_map`/`text`/`u64` as defined; `TempAppData` matches between Task 7's module and test.
- **Known risks, accepted:** the pinned digests assume the implementation matches this plan's encoding byte-for-byte — a mismatch fails the pin test, which is the pin doing its job (recompute only after confirming the divergence is intentional); `num-bigint`/`num-rational` API details (`magnitude()`, operator reference forms) are from the 0.4 line — if a signature differs, adjust the call site, not the contract; Vitest is installed at latest rather than a guessed pin, with `package-lock.json` as the pin.
