# src-tauri

Rust backend for the Tauri application. See [../docs/technical-design.md](../docs/technical-design.md)
for the complete architecture; this file is a quick orientation, not a substitute.

## Building and running

Run these from `src-tauri/` unless noted otherwise.

- `cargo build` — compile the library and binary.
- `cargo test --features test-support` — run the Rust test suite. `test-support` enables
  fixture and temp-directory helpers (`testsupport`, `source::fixture`) that production
  builds never enable; CI always passes this flag.
- `cargo test --features test-support --test acceptance` — the acceptance harness alone.
- `cargo test --features test-support corpus_conformance -- --ignored --nocapture` — the
  whole-corpus parser conformance run against an installed Stellaris and ACOT. Ignored by
  default, so ordinary `cargo test` and CI never need the corpora; a missing corpus root
  fails the run rather than skipping it. **Re-run it on a Stellaris build change, a Jomini
  upgrade, or any edit to the dialect lexer** — the same standard `docs/adr/0008` holds for a
  texture-decoder upgrade. Records and drift live in `docs/conformance/parser/`; see
  `src/analysis/parser/conformance/` for the contract.
- `cargo test --features test-support parse_and_resolve_conformance -- --ignored --nocapture`
  — the drift-checked parse-and-resolve run (Phase 4M) against the same installed corpora:
  every declared Resolution Profile row and the localization file stream, with refusals and
  per-cell visible-failure counts recorded rather than erroring. **Re-run it whenever the
  parser run re-runs, and additionally on an ACOT update or any Resolution Profile change.**
  Same record directory; see `src/analysis/resolver/conformance.rs` for the contract.
- `cargo clippy --all-targets --features test-support -- -D warnings` — lint, matching CI.
- `cargo fmt` — format.
- `npm run app:dev` (from the repo root) — run the desktop app with hot reload, under the
  development identity. Prefer this over `npm run tauri dev`; see below.
- `npm run app:verify` (from the repo root) — a debug build with the frontend embedded. **This is
  the only loop that applies the Content Security Policy.**
- `npm run tauri build` (from the repo root) — produce a release bundle.

Rust unit tests are co-located with the code they exercise in inline `#[cfg(test)] mod tests`
modules. Use `tests/` integration targets only when compiling as an external crate is itself part of
the contract, as it is for the acceptance harness and test-support boundary.

CI (`.github/workflows/ci.yml`) runs format, clippy, and tests on both macOS and Windows —
Windows matters because `durability.rs` has a `#[cfg(windows)]` arm that only compiles and
executes there.

### The acceptance harness

`tests/acceptance/` is the golden-case vehicle (docs/technical-design.md, "Verification
architecture"): fixture Source Snapshots → the build use case → a published revision → the
desktop read, headless, booted through `composition::open_stores` exactly as the composition
root boots. Its `main.rs` doc comment is the contract — read it before widening the harness,
because two things about the layout are not guessable. It is **one** target whose sibling files
are modules of it, so a new `tests/*.rs` file beside it is a separate crate and cannot reach the
harness; and it sees only the crate's `pub` surface, which is why it asserts on application DTOs
rather than on the private projections a Tauri command returns.

### The development identity overlay

`tauri.dev.conf.json` overrides one key, `identifier`, to `com.jackson.stellaris-docs.dev`. That
single override isolates both things that matter: the application-data directory Tauri derives from
the identifier, and the key `tauri-plugin-single-instance` locks on. The design requires this —
"development tools and automated tests must use explicitly separate temporary application-data
directories and either omit the packaged plugin or use isolated application identifiers… this
isolation is a caller precondition, not a guarantee supplied by the plugin".

Three things about it that JSON cannot carry as comments:

- **The overlay is inert unless passed explicitly.** Tauri auto-merges only `tauri.conf.json` and
  `tauri.<platform>.conf.json`. The `app:dev` and `app:verify` scripts pass it with `--config`;
  anything else you run must too, or you silently get a second application-data directory and
  yesterday's published revision appears to have vanished.
- **It must contain nothing but the identity.** Config merge is RFC 7386, which replaces arrays
  wholesale, so an `app.windows` entry here would discard the base window's title and size rather
  than adding to them. An `app` key at all could also weaken the security policy. The gate in
  `composition::config_policy` enforces both.
- **Switching between configurations forces a rebuild.** The CLI passes the merged config through
  `TAURI_CONFIG`, which `tauri-build` declares as `rerun-if-env-changed`, so alternating between
  `app:verify` and a plain `cargo build` recompiles the Tauri context each time.

### Content Security Policy

`npm run tauri dev` enforces **no** CSP. Tauri injects the policy only when it serves the embedded
assets itself, and `build.devUrl` points the webview at the Vite dev server instead. `app:verify`
builds with the frontend embedded, which is what makes `app.security.csp` apply — so a policy
regression is invisible in the fast loop and only appears in that one. `composition::config_policy`
checks the configured string on every `cargo test`; it does not and cannot prove the webview
enforced it.

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
| `revisions` | Sole owner of Documentation Revision bundle I/O: staging, validation, atomic publication (two commit points: durable bundle move, then compare-and-swap of the state publication reference), and the read side — pinned typed Revision Readers, and the retirement claim Phase 9's retention sweep will drive. |
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
