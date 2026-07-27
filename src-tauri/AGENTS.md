# src-tauri

Rust backend for the Tauri application. See [../docs/technical-design.md](../docs/technical-design.md)
for the complete architecture; this file is a quick orientation, not a substitute.

## Building and running

Run these from `src-tauri/` unless noted otherwise.

- `cargo build` — compile the library and binary.
- `cargo test --features test-support` — run the Rust test suite. `test-support` enables
  fixture and temp-directory helpers (`testsupport`, `source::fixture`) that production
  builds never enable; CI always passes this flag.
- `cargo clippy --all-targets --features test-support -- -D warnings` — lint, matching CI.
- `cargo fmt` — format.
- `npm run tauri dev` (from the repo root) — run the whole desktop app with hot reload.
- `npm run tauri build` (from the repo root) — produce a release bundle.

CI (`.github/workflows/ci.yml`) runs format, clippy, and tests on both macOS and Windows —
Windows matters because `durability.rs` has a `#[cfg(windows)]` arm that only compiles and
executes there.

## Module map

Each module's own `mod.rs` doc comment is the authority; this table is a shortcut. Names in
quotes are section headers in the technical design doc.

| Module | Owns |
| --- | --- |
| `canonical` | Shared leaf primitives every stable identity builds on: domain-separated digests over a tagged length-prefixed encoding, logical relative paths, exact numerics. Not itself a named deep module. |
| `source` | Complete Mod Source traversal and content identity: enumeration, logical-path normalization, hashing, fingerprints, build-lifetime Source Snapshots, live-source re-verification before publication. |
| `discovery` | Finding Stellaris and Mod Installations and reading the metadata needed to populate the Mod Library. |
| `state` | The durable mutable state document: schema, atomic replacement, quarantine recovery, the narrow publication-reference capability. |
| `analysis` | Turns Source Snapshots plus asset-materialization outcomes into a finalized Revision Candidate: parser adaptation, content-type resolution, documentation generation, Source Excerpt capture. |
| `assets` | Byte-conversion mechanics behind the shared content-addressed Asset Store: DDS classification, the pinned conversion recipe, typed materialization outcomes, blob publication. |
| `localization` | The Stellaris localization language: ingestion, markup tokenization, fallback, Static Localization Reference resolution, plain-text projection, display tokens. |
| `search` | Both sides of the persisted search contract: deterministic index construction, the versioned index representation, query normalization/matching/ranking. |
| `revisions` | Sole owner of Documentation Revision bundle I/O: staging, validation, atomic publication (two commit points: durable bundle move, then compare-and-swap of the state publication reference). |
| `companion` | Pairing secrets, Companion Sessions, listener lifecycle, Companion-Ready access policy, trusted companion revision handles. |
| `application` | Named product use cases spanning deep modules: documentation reads, and the Ensure/Rebuild/Validate workflows. |
| `transport` | Input adapters — Tauri commands and Companion HTTP requests — mapped into shared application DTOs. Calls application use cases only; never implements parallel rules. |
| `composition` | Composition root: constructs concrete modules, process-lifetime shared state, background execution resources, and the Tauri `App`. The only place framework types (besides `transport`) and application modules meet. |
| `durability` | What making a directory entry durable means per platform (fsync vs. `FlushFileBuffers`), shared by `state::replace` and `revisions` publication so the fact has one answer, not two (`docs/decision-log.md`, D-123). Its `#[cfg(windows)]` arm is why CI runs a Windows job. |
| `error` | Error conventions shared by every module: typed `Result<T, E>` per operation for expected outcomes, `Unexpected` with a correlation identifier for invariant violations and defects. Panics are never control flow. |
| `testsupport` | Test-only helpers behind the `test-support` feature (e.g. isolated temp app-data directories). Fixture Source Snapshots live in `source::fixture` instead, since building one uses the same construction path a live snapshot does. |

Dependency direction (enforced, not just conventional): transports → application → deep
modules → filesystem/parser/image-decoder/persistence adapters. Application modules never
import Tauri; only `transport` and `composition` do.
