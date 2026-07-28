# Implementation Plan — Outline

Status: Outline for review

Last updated: 2026-07-26

> **For agentic workers:** This is the master outline and the durable planning document.
> Each phase gets a deep pass before execution that pins contracts, public surfaces, test
> obligations, pinned vectors, and evidence-bearing decisions — not implementation bodies,
> which are delegated to capable implementing agents. Phase plans are working artifacts:
> once a phase is implemented and merged, the plan document is deleted (the code is the
> authority; git history preserves the plan).

**Goal:** Implement the MVP defined by the [product requirements](./product-requirements.md) and [technical design](./technical-design.md): a Tauri desktop app that builds deterministic, provenance-preserving technology documentation for one Target Mod against Vanilla Content, readable on the desktop and through read-only Companion Mode, accepted by the five golden cases.

**Architecture:** One Rust process, one Cargo package, module map `composition / transport(tauri,http) / application / discovery / source / analysis / localization / search / revisions / assets / state / companion`. Adapters call application use cases; deep modules own their contracts. A build turns Source Snapshots into an immutable, atomically published Documentation Revision bundle read by both transports.

**Tech Stack:** Rust + Tauri 2, Jomini (`TokenReader` only, wrapped), `image_dds` 0.7.2 + `png` 0.18.1 (pinned recipe), React + TypeScript + Vite, TanStack Router, Tailwind CSS, shadcn/ui, `@xyflow/react` (deferred until a graph earns it).

## Global constraints

Copied from the accepted technical design; every phase inherits these.

- One Cargo package, one Rust process; no workspace crates, sidecars, or child processes.
- Dependency direction: transports → application → deep modules → filesystem/parser/image/persistence adapters. Application modules never import Tauri or the HTTP framework.
- Jomini is consumed only through its `TokenReader` incremental lexer, behind the private parser adapter; parser-library types never escape.
- Asset conversion uses only the pinned recipe: `image_dds` 0.7.2 (`default-features = false`, `ddsfile` only), `Surface::decode_layers_mipmaps_rgba8` mip 0 / one layer, untagged sRGB PNG via `png` 0.18.1; production graph keeps `image_dds` encoding and ISPC disabled.
- All stable identities (fingerprints, Revision identifiers, Entry Keys, Hidden Route identities, asset keys) use SHA-256 over domain-separated canonical encodings; binary floating point never participates in identity.
- Durable mutable state is one versioned JSON document with atomic replacement, quarantine recovery, and a newer-schema blocking screen.
- Expected outcomes cross transports in the `{ "ok": true|false }` Result envelope with per-operation error unions; the Result type is vendored locally until the package is published.
- `tauri-plugin-single-instance` registered before every other plugin.
- Frontend: no global state library, no TanStack Query, no SSR; `@tauri-apps/api` imports confined to the Tauri documentation adapter and desktop control module.
- CSP release gate: no packaging until desktop and Companion CSP suites, boundary limits, redaction, and asset-scope tests pass against production artifacts.
- Functional MVP targets macOS; release packaging covers macOS `.dmg`, Windows NSIS, Linux AppImage + `.deb`.
- TDD throughout: failing test first; regression tests proved to fail before the fix; negative controls for gates and generators.

## Starting state

- `src-tauri/` is a fresh Tauri 2 scaffold (no production modules; CSP currently disabled — gated before release).
- `src/` is the default Vite React scaffold.
- `tools/` holds the four completed spike harnesses (parser, oracle, DDS, bundle) — reference implementations to mine, not production code.
- `fixtures/` holds parser, oracle, bundle, and DDS fixture corpora with READMEs.
- Docs: PRD, technical design, resolver/parser/DDS/revision-bundle spikes, ADRs 0001–0009, MVP acceptance, decision log, CONTEXT.md glossary.

## Sequencing rationale

Two ideas shape the order:

1. **Prove the risky seams first.** The atomic publication chain (state pointer ↔ bundle move ↔ readers), source snapshot consistency, and the Tauri/HTTP dual-transport contract are where late integration would hurt most. A walking skeleton (Phase 3) drives a stub analysis through real state, source, revisions, and a real Tauri read before any deep analysis exists. From then on, every phase lands inside a working end-to-end harness instead of waiting for a big-bang integration.
2. **Deep modules land in dependency order, each behind its accepted contract.** Parser → resolver → localization → generator → search/assets → workflows → frontend → companion. Search (Phase 7) and Assets (Phase 8) have no dependency on each other and can proceed in parallel once Phase 6 fixes the Analysis Draft shapes.

The five golden cases are the acceptance backbone: fixture authoring starts early (Phase 4) and each later phase widens which golden assertions pass. The pinned "ordinary drawable vanilla technology" is selected in Phase 4 when resolver fixtures are authored.

## Ticketing

In a solo project where an agent implements, a ticket is a unit of the maintainer's judgment, not a unit of work: its boundary sits where a human review gate is genuinely valuable — where one could reject the work while accepting its neighbor. Tickets stay thin pointers to plan slices with a done-when; the detailed phase plans remain the spec, and plan content is never duplicated into tickets.

Granularity follows decision density, not size:

1. **Mechanical, fully specified** (Phase 0; localization tokenization; the pinned DDS recipe) — one ticket per phase or task cluster, one-shot, review at the end.
2. **Contract-defining** (state replacement protocol, Source Snapshots, publication protocol) — one ticket per contract, reviewed before dependents build on it, because a defect cascades.
3. **Judgment-heavy, evidence-decided** (Resolution Profile rows, generator route semantics, golden cases) — smaller tickets aligned to units of evidence (oracle records, policy rows, golden slices), each deserving the focused review a PR forces, even when small.

Blocking relations encode the dependency DAG so parallelizable work is visible. The five golden cases are tracked as milestones, not tickets. First-pass reviews may be delegated to a subagent; the maintainer's attention is reserved for evidence-bearing decisions — oracle records, Resolution Profile rows, and anything that changes a pinned digest.

---

## Phase 0 — Foundations (implemented)

**Deliverable:** A compiling module skeleton with shared primitives every later phase consumes, plus test and CI infrastructure.

1. Rust package layout per the accepted module map; composition root; executable target that only starts the library.
2. Error conventions: typed expected `Result<T, E>` vs. unexpected internal error channel; correlation identifiers; panic policy.
3. Canonical encoding module: logical path normalization (NFC, `/`, case-preserving, rejection rules), canonical UTF-8 byte ordering, domain-separated SHA-256 helpers, canonical map/set/sequence encoding rules.
4. Exact numeric representation: lexeme-preserving arbitrary-precision rationals with deterministic static arithmetic.
5. Analysis version vector type with its named components.
6. Test infrastructure: isolated temporary application-data directories, fixture-corpus helpers, cargo test + Vitest + CI pipeline (fmt, clippy, tests).

**Exit:** Skeleton compiles on macOS CI; canonicalization and numeric property tests pass; a negative control proves the CI gate can fail.

## Phase 1 — Durable state and discovery (implemented)

**Deliverable:** The state module and Mod Library derivation, headless.

1. State module: versioned JSON schema, application-owned parsed state type, atomic replacement protocol, `CommittedDurabilityUncertain` reconciliation, serialized mutations.
2. State evolution: quarantine of malformed state, recovery flow states, newer-schema blocking condition (as application state; UI in Phase 10).
3. Narrow publication-reference mutation capability (consumed by `revisions` in Phase 3).
4. Discovery Location identity and configuration; rebind vs. remove semantics.
5. Mod Installation identity derivation (location id + normalized relative path); collision visibility.
6. Discovery module: Stellaris installation and mod scanning, descriptor metadata, derived Mod Library with unavailable-location behavior.

**Exit:** Crash-injection tests around every replacement step; identity property tests (rename, rebind, case-only, collision); discovery behavioral tests over fixture directory trees.

## Phase 2 — Source module

**Deliverable:** Source truth: snapshots, fingerprints, and the fixture seam the whole verification architecture stands on.

1. Deterministic enumeration of analysis-relevant files; logical path normalization and escape/symlink containment enforcement.
2. Read + hash exact bytes; Target Mod and Vanilla Content fingerprints.
3. Source Snapshot capability, including lazy frozen capture for large binary assets.
4. Final live-source verification (the pre-publication re-fingerprint), including referenced-source-asset sets.
5. Disposable filesystem-metadata accelerator (hint only; never identity).
6. Source-owned test support: construct snapshots from fixture corpora (this is what golden tests and every analysis test consume).

**Exit:** Behavioral tests for edits/additions/deletions/renames/ordering/rejection/mid-build change; fingerprint determinism across enumeration order and parallelism.

## Phase 3 — Walking skeleton

**Deliverable:** The smallest honest end-to-end thread: hand-authored Revision Candidate → real bundle → real atomic publication → real Tauri read → minimal React page.

1. Minimal revision bundle: manifest with real canonical Revision identifier, one trivial "entry list" document, schema validation, required-entry hashes. (implemented)
2. `revisions` publication protocol: staging on same filesystem, validation, atomic move, state pointer commit via the Phase 1 capability, crash-point injection. (implemented)
3. Minimal Revision Reader with handle pinning. (implemented)
4. Test-only candidate provider: the build coordinator accepts a Revision Candidate provider seam; the skeleton supplies hand-authored candidates from test support, exercising application coordination and publication without pretending to analyze Stellaris source. No false analysis behavior enters the production `analysis` module; deleting this provider is an explicit Phase 6 entry condition.
5. One awaited Tauri build command + one read command using the Result envelope and vendored Result package boundary. Includes a negative test that a serialized failure cannot contain `Unexpected` message detail — `error::Unexpected` derives `Debug` including the message, so redaction must be enforced structurally at the transport seam, not by convention (Phase 0 review finding).
6. Frontend bootstrap: Vite + TanStack Router + Tailwind + shadcn shell; documentation-client interface with the desktop (Tauri) adapter; one page listing entries.
7. `tauri-plugin-single-instance` registered first in the composition root; development tools and tests use isolated application-data directories and identifiers per the design's caller precondition. (Packaged platform validation stays in Phase 12.)
8. Baseline desktop CSP enabled with the React shell: same-origin defaults, no remote origins, no `unsafe-eval`. Later phases extend it (asset protocol in Phase 10, Companion policy in Phase 11); the production-artifact release gate stays in Phase 12.
9. Acceptance-harness skeleton: fixture snapshots → build use case → published revision → desktop read, in the shape the golden tests will keep.

**Exit:** The thread runs in the real app window under the baseline CSP and in the headless harness; publication crash-injection suite passes; the harness shape is reviewed as the golden-case vehicle.

## Phase 4 — Parser and resolver

**Deliverable:** Real parsed model and the two-contributor resolver with oracle-backed Resolution Profile rows.

**Before detailing this phase:** derive a resolver-support checklist from the five golden cases — every registry row needed to find and interpret technologies, events and Grant Sites, megastructures, buildings, ship components, sprites, scripted triggers/effects/variables, and localization must be named. A content category may not enter Phase 6 until every row it requires is explicit and either implemented or visibly failing.

1. Application-owned parsed representation (`ParsedFile`: ordered definitions, exact byte ranges, operators, mixed containers, unknown constructs, scripted-constant references, Clean/Recovered evidence).
2. Jomini `TokenReader` production adapter: range derivation and verification, fault resynchronization, Stellaris-dialect lexing; port validated logic from the parser spike.
3. Whole-corpus verification: range re-slicing, deterministic parsed-model digests, malformed-fixture recovery behavior.
4. Resolution Profile implementation: versioned registry policies — technology whole-object replacement (incl. omitted-`potential`), sprite and script global path-order streams, localization source/`replace/` stream, inline-script textual expansion, scripted variables; unresolved cells fail visibly.
5. Provenance model: contributed, inherited, defaulted, duplicate, shadowed facts per field.
6. Game-oracle harness integration: pinned oracle records as resolver test expectations; version-change blocking rule.
7. Golden fixture authoring: pin the ordinary drawable vanilla technology; author small license-clean redefinition, malformed, zero-weight, and Enigmalith-shaped fixture mods, committed for normal CI. No Vanilla or mod source is redistributed.
8. Local-corpus acceptance harness: drift-checked runs against the installed Vanilla and ACOT (logical paths + checksums recorded, following the DDS-spike record pattern). These runs are a required acceptance activity from this phase onward; a failure after a game or mod update is signal, not noise.

**Exit:** All oracle records pass against the resolver; resolver fixtures cover every claimed-supported registry row; golden case 5 (redefinition provenance) passes at the resolver seam; a local-corpus parse-and-resolve run over installed Vanilla and ACOT completes with recorded drift state.

## Phase 5 — Localization

**Deliverable:** The localization deep module, build-side and read-side.

1. Localization-file ingestion, locale identity, and per-language tables.
2. Markup tokenization into application-owned display tokens; style/color markers, known inline icons, visibly raw runtime tokens and unknown markup.
3. Selected-language → English → raw-key fallback.
4. Static Localization Reference resolution with cycle detection.
5. Cited-key transitive closure across all languages (build stage owned by `analysis`, semantics owned by `localization`), with the truncated-closure negative controls.
6. Plain-text projection for search indexing.
7. Effective-language derivation: explicit override → detected Stellaris language → English; macOS Documents-denial behavior as a typed condition.

**Exit:** Tokenization/fallback/cycle suites pass; closure-equivalence test (expanded text identical to a preserve-everything model) with proven-failing negative control.

## Phase 6 — Technology documentation generation

**Deliverable:** The real `analysis` pipeline: Analysis Draft → finalize → Revision Candidate.

**Entry conditions:** the Phase 3 test-only candidate provider is deleted from the build path (its hand-authored candidates may survive only as `revisions`-module test fixtures), and the Phase 4 resolver-support checklist is closed for every category this phase documents.

1. Technology entry generation: prerequisites, eligibility (All of/Any of/negated groups), blockers, Base Draw Weight, Weight Modifiers with `×0` prominence, unlocked content, Resolved Base Values via exact numerics, runtime/ambiguous labeling.
2. Grant Site discovery: enclosing-action model, Unlock Effect normalization (add option / add progress / complete / weight change), one route card per Grant Site with combined co-located effects, Player-Facing Anchors, Route Summaries, technical trace.
3. Hidden Route identity: canonical structural digest, identical-sibling ordinal rule, stability suite (whitespace/comments/range-shift/commutative-reorder invariance; semantic-change divergence).
4. Analysis Issues: evidence-dependency graph, four impact kinds, propagation strictly along recorded edges; typed unsupported/unresolved facts.
5. Bounded Source Excerpts: 16 KiB line-aligned capture, visible truncation, undecodable-byte projection, no arbitrary-range reads.
6. Thin Searchable Entries for megastructures, buildings, ship components with direct technology gates.
7. Analysis Draft / asset-request surface / `finalize` contract (placeholder substitution lands with Phase 8; finalize ships first with a no-asset fixture path).

**Exit:** Golden cases 1, 2, and 4 pass through the harness (minus icons); Enigmalith fixture produces separate Databank and Final Spark route cards with correct Unlock Effects; route-identity and issue-propagation suites pass.

## Phase 7 — Search

**Deliverable:** The deep `search` module used at build and read time. Parallelizable with Phase 8.

1. Versioned index representation, encode/decode, and the build-stage construction call from `analysis` (effective language + English).
2. Pinned Unicode normalization (NFKC, case fold, whitespace collapse).
3. **Resolve open decision:** typo-tolerant matching algorithm and any index shape it needs, against representative typo fixtures.
4. Deterministic ranking: exact > prefix > fuzzy/identifier, stable-identity tie-break; category and Vanilla-inclusion filters; bounded limits.
5. On-demand in-memory index construction for non-materialized languages; bounded byte-capped disposable cache.
6. Request-generation suppression contract (client side lands in Phase 10).

**Exit:** Round-trip, ranking, determinism, and bounds suites; multi-category Enigmalith search assertions pass at the module seam.

## Phase 8 — Assets

**Deliverable:** The asset module and shared content-addressed Asset Store. Parallelizable with Phase 7.

1. Application-owned DDS container reader: classification (`MalformedMedia` vs. `UnsupportedFormat`) before decode; layer/alpha/size policy refusals.
2. Pinned `image_dds` adapter and PNG encoder per the recipe; recipe value type with resolved dependency versions.
3. Asset keys (source-byte hash + canonical recipe); Asset Store staging, atomic blob publication, trusted metadata, content validation proof.
4. Typed materialization outcomes; injected `ConversionFailure` coverage; `analysis::finalize` placeholder substitution and scoped issues (completing the Phase 6 contract).
5. Sprite/icon reference resolution in `analysis` (which asset belongs to which entry) via Source Snapshot reads.
6. Garbage-collection participation: live-set derivation, conservative failure, grace period.

**Exit:** Full DDS fixture matrix (valid/unsupported/malformed/missing) yields exact typed outcomes; independent second-reading cross-check harness wired for decoder upgrades; golden case 1 icon assertions pass.

## Phase 9 — Application workflows and desktop transport

**Deliverable:** The real product workflows over the assembled pipeline, exposed as the complete Tauri command surface.

1. Ensure / Rebuild private coordinator: build lease, snapshot protocol steps 1–7, double-fingerprint verification, `SourceChangedDuringBuild` discard, cache-hit path with authoritative freshness check.
2. Validate Published Revision read-only diagnostics.
3. Documentation status derivation (availability × artifact × freshness × integrity × completeness × build state × access) with the fixed desktop priority; startup background verification worker; Refresh join semantics.
4. Revision retention: superseded-bundle retirement after handle release, startup sweep ordering after state recovery.
5. Complete Tauri command surface and DTOs for every documentation-client operation and build outcome union.
6. Cross-language contract suite (shared cases, negative controls for discriminants/shapes/JSON-safety) — HTTP side joins in Phase 11.

**Exit:** Golden cases run through real Ensure with cache-hit and invalidation assertions (edit/add/delete/rename/asset-byte/version-vector); `BuildInProgress`, staleness, and status-derivation suites pass.

## Phase 10 — Desktop frontend

**Deliverable:** The complete desktop experience over the documentation client.

1. First-launch setup wizard: detection, path correction, confirmation, macOS Documents-access notice and retry.
2. Mod Library page: unified locations, per-installation status states, advisory compatibility/dependency warnings, unavailable locations.
3. Search combobox: lazy bounded requests, generation suppression, category/Vanilla filters, result typing and disambiguation, narrow-screen dialog presentation.
4. Technology page: localized name/description/icon via display tokens, structured requirement groups, weight sections with `×0` prominence, unlock content, route cards with summaries and technical trace, provenance, entry-scoped issues, excerpt viewer.
5. Route visibility: persistent desktop hide/restore/reset over stable identities; changed-route reappearance.
6. Desktop control module: builds, Refresh, Discovery Location management, language override; router invalidation after host mutations.
7. Asset delivery: scoped asset protocol, opaque URLs, placeholder rendering; extend the Phase 3 baseline CSP with `asset:` / `http://asset.localhost` in `img-src` only.
8. Status, stale-revision warning, Incomplete Documentation banners, recovery and newer-schema screens; route-level error boundaries with correlation identifiers.
9. Component/accessibility/responsive suites; loaders with explicit `gcTime`/`staleTime`.

**Exit:** All five golden cases pass their desktop-visible assertions through the real UI on macOS; responsive usability checks pass at phone width.

## Phase 11 — Companion Mode

**Deliverable:** The read-only companion path, end to end.

1. `companion` module: pairing-secret lifecycle (rotation, expiry, attempt limits, constant-time compare), Companion Sessions (cap 8, cookie semantics, invalidation on disable/exit).
2. Listener lifecycle: bind, address enumeration and selection, "waiting for first connection" state, troubleshooting copy.
3. Companion access module: readiness observations, published-revision selection, trusted companion revision handles; never hashes on the read path.
4. HTTP transport: the seven-endpoint surface, untrusted input parsing, Result envelope, status-code policy, boundary limits, same-origin and `Host`/`Origin` enforcement, cache-control headers, redaction.
5. HTTP documentation adapter in the frontend; QR pairing flow with fragment handling; session-memory language and route-visibility overrides via capabilities.
6. Companion cache library and switcher: the Mod Library view on a Companion Device shows Ready, Needs build, and Out of date states, switches among Companion-Ready Caches without changing the desktop's active Target Mod, and tells the user to build on the Desktop Host when a cache is missing or stale (user stories 80–82).
7. SPA shell serving for frontend paths; authenticated asset route; companion same-origin CSP variant of the Phase 3 baseline.
8. Security suite: pairing rotation, stale cookies, oversized/slow requests, concurrency limits, read-only boundary, path privacy.

**Exit:** Every golden case passes its equivalent companion read (acceptance requirement 6); a companion session switches between two Ready caches and receives the build-on-desktop guidance for a stale one; the full companion security suite passes.

## Phase 12 — Acceptance hardening and packaging

**Deliverable:** The release-eligible MVP.

1. Full-program verification passes not already owned by a phase: reproducibility (roots/enumeration/schedule), metamorphic suite, crash-recovery matrix, complete status-derivation matrix; final drift-checked local-corpus acceptance run against installed Vanilla and ACOT.
2. CSP release gate: exercise the accumulated desktop and Companion CSPs (Phases 3, 10, 11) against production artifacts; controlled-token rendering; asset-scope tests.
3. Single-instance packaged validation: platform adapters on real machines, macOS stale-socket recovery.
4. Packaging: macOS `.dmg`, Windows NSIS, Linux AppImage + `.deb`; production dependency-graph check (no `image_dds` encoding/ISPC).
5. Real-machine smoke tests per platform: startup, replacement semantics, history fallback, asset scope, one DDS conversion per delivery path, Companion + firewall, Documents allow/deny (macOS).
6. Residual product polish: Unlock Path diagram decision (only if Enigmalith evidence demands it), MIT licensing text, project-name placeholder resolution.

**Exit:** MVP acceptance document satisfied end to end; release gate green on packaged artifacts.

---

## Cross-cutting workstreams

These run inside phases rather than as separate phases:

- **Golden fixtures** are authored in Phase 4 and widened through Phases 5–8; every phase states which golden assertions it turns green.
- **License-clean fixtures vs. local corpora:** committed fixtures are small, license-clean, and drive normal CI; installed Vanilla and ACOT are exercised through drift-checked local-corpus acceptance runs (Phase 4 onward, final pass in Phase 12) whose records hold logical paths and checksums, never redistributed source. Both are required: fixtures alone could pass a convincing imitation without processing the content that motivated the app.
- **Crash and failure injection** lands with the module that owns the commit point (state Phase 1, publication Phase 3, assets Phase 8, workflows Phase 9).
- **Negative controls** accompany every gate and sophisticated test when that gate is written, per the engineering principles.
- **Spike code migration:** `tools/` harnesses are mined for validated logic (parser adaptation, DDS classification, bundle round-trips) but production modules are written fresh behind their designed interfaces; spike harnesses that remain useful (oracle runner, DDS cross-check) stay in `tools/` as verification tooling.

## Open decisions resolved during planning or implementation

| Decision | Resolves in |
| --- | --- |
| Typo-tolerant search algorithm and index representation | Phase 7, task 3 |
| Final serialized fields for operation-specific Result payloads | Phase 9 detailed plan |
| Pinned ordinary drawable vanilla technology | Phase 4, task 7 |
| Unresolved Resolution Profile cells | Phase 4 (resolver-backed tests close named cells; unclosed cells stay visibly failing) |
| Graph layout implementation (`@xyflow/react` + Dagre) | Phase 12, task 6 — only with Enigmalith evidence |
| Project name and copyright wording | Phase 12, task 6 |
