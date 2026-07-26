# Revision bundle evaluation

Status: Planned

## Decision

Determine whether build-time denormalized JSON is a suitable structured-data format for immutable Documentation Revision bundles, or whether the MVP should use SQLite.

## Corpus

Evaluate:

- Vanilla Content from the base-game file set, plus representative referenced assets supplied by the Stellaris Installation.
- Ancient Cache of Technologies.
- Gigastructural Engineering.
- The malformed-source golden fixture.
- The technology-redefinition game-oracle fixture.

The spike must use generated documentation-shaped data rather than raw Mod Source sizes alone.

## JSON prototype

Build one immutable revision bundle per corpus case with:

- A versioned manifest and required-entry hashes.
- Category browse indexes.
- Per-language search material.
- Documentation records.
- Localization dictionaries.
- Analysis Issues and revision diagnostics.
- Bounded Source Excerpts.
- Logical asset references and a complete required-key set.

Populate a shared content-addressed Asset Store from representative Vanilla and mod DDS inputs. Key each output by source bytes, explicit conversion-recipe version, output format, and conversion parameters.

Generate every denormalized view from one canonical in-memory documentation model. Compare one-file-per-document with simple category or stable-identity-prefix sharding when file count is material.

## Measure

For each bundle shape, record:

- Total bytes and file count.
- Bytes attributable to duplicated materialized fields and localization.
- Cross-revision localization bytes recoverable through content-addressed shared chunks.
- Unique Asset Store bytes, per-revision referenced bytes, and deduplication ratio.
- Cold conversion time and warm rebuild time with unchanged assets.
- End-to-end fingerprint, parse, resolve, generate, asset-convert, write, validate, and publish time.
- Time spent in each build phase.
- Cold and warm revision-open time.
- Memory retained for the active search and browse indexes.
- Cold and warm search latency.
- Cold and warm documentation-record latency.
- Companion response size and time for representative search and documentation reads.
- Cleanup behavior for abandoned staging and unreferenced complete bundles.

Test on macOS during MVP development. Preserve the corpus and measurement procedure so Windows and Linux packaging tests can identify filesystem-specific regressions before release.

## Decision budgets

Declare these budgets before collecting the deciding measurements. Record at least 30 measured iterations after warm-up and report median, p95, maximum, and measurement-machine details. “Cold” clears application-owned caches and starts a new process; it does not claim to evict the operating system's filesystem cache.

The largest corpus must satisfy:

| Property | Budget |
| --- | ---: |
| Cold revision open through validated Revision Reader | p95 ≤ 500 ms |
| Warm host search, maximum result limit | p95 ≤ 100 ms |
| Cold host search after revision open | p95 ≤ 250 ms |
| Warm documentation-record read | p95 ≤ 100 ms |
| Cold documentation-record read after revision open | p95 ≤ 250 ms |
| Incremental retained memory for active browse and one language's search indexes | ≤ 256 MiB |
| Files in one revision bundle | ≤ 10,000 |
| Complete bundle validation | p95 ≤ 2 s |
| Bundle size excluding shared Asset and Localization Stores | ≤ 2.0× the canonical unsharded read-model payload and ≤ 1 GiB |

A budget may change only before the format outcome is recorded, with a written user-impact rationale and the replacement threshold. Failing a budget selects sharded JSON, a shared Localization Store, or SQLite according to the measured cause; it is not waived by calling the result responsive.

## Correctness checks

Verify that:

- The manifest detects missing, changed, and unexpected required entries.
- A reader cannot address staging or unreferenced bundles.
- Browse summaries, search summaries, and full records agree on stable identity, category, provenance, and localization keys.
- Language switching and English/raw-key fallback do not require rebuilding.
- The malformed-source case publishes Incomplete Documentation rather than a partially written bundle.
- Removing disposable parser or resolver artifacts does not affect revision reads.
- Every asset reference resolves to a required Asset Store key.
- Asset keys change when source bytes or conversion behavior changes and remain stable otherwise.
- Garbage collection preserves shared live blobs and removes unreferenced blobs.
- A referenced source-asset byte change invalidates an otherwise unchanged revision, while an unrelated binary asset change does not.
- Runtime Asset Store cleanup eventually removes unreferenced blobs without racing recently issued asset URLs.

## Outcome

Accept materialized JSON when the largest corpus meets every declared budget without loading complete documentation into memory. Prefer sharded JSON when it resolves file-count or oversized-file problems while meeting the same latency and memory budgets. If preserved localization dominates cross-revision duplication, evaluate immutable content-addressed localization chunks before replacing the complete bundle format.

Choose SQLite when JSON or sharded JSON misses a budget because of retained memory, validation or filesystem overhead, or database-like indexing and query machinery in application code.

Use the end-to-end timings to decide the build invocation model. Prefer an awaited asynchronous Tauri command when p95 complete builds for every representative Target Mod are at most three seconds and navigation does not need to outlive the invocation. Introduce an explicit host-owned job when that budget is missed or navigation must preserve progress independently.

Record the measurements, chosen bundle shape, and decision here before implementation planning.
