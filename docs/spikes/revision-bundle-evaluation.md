# Revision bundle evaluation

Status: Complete against Stellaris `Pegasus v4.4.6`. Materialized JSON is adopted, one file per document, carrying only the localization the documentation cites. Every declared budget is met. No shared Localization Store, no sharding, no SQLite, and no host-owned job.

The hypothesis was that build-time denormalized JSON would be fast enough and that preserved all-language localization *might* dominate cross-revision duplication. The first held with room to spare. The second held so overwhelmingly that it turned out to be asking the wrong question.

JSON is not close to its latency budgets — cold revision open is 18 to 35 ms against 500, warm search is 0.22 ms against 100, retained index memory is under a megabyte against 256. The read-side question was never the real one.

Localization does not merely dominate; it *was* the artifact. A Gigastructures revision serialized with every key is 191 MiB, of which 189 MiB is localization and 2.4 MiB is documentation. Six revisions carry 959 MiB between them and hold 353 MiB of distinct content.

The measurement that resolved it is that **the documentation cites 1.15% to 1.45% of those keys**. Preserving the cited set plus the closure of its static references — every available language, inside the bundle — costs 1.74 to 2.59 MiB. Every budget then passes, the shared store the design pre-authorized stops being necessary, and the build phase that store required stops existing, which is what brought builds back inside the three-second threshold.

This spike spent most of its effort measuring how to store 150 MiB per revision that no reader ever reads.

## Decision

Materialized JSON, one file per document, self-contained.

- **A revision preserves the localization its documentation cites**, plus the closure of that set's static references, in every available language. 1.74 to 2.59 MiB, inside the bundle. Not the complete tables, and not a live read against Mod Source.
- **Search material covers the selected language and English.** Product requirements 21 and 28 name both; one is not a permitted reading of either.
- **The build is an awaited asynchronous Tauri command.** p95 complete builds are 1.8 to 2.3 seconds against the declared 3-second threshold.

Three mechanisms the design held open are measured out rather than left unmentioned:

**No Localization Store.** `docs/technical-design.md:349` pre-authorized a shared content-addressed store conditional on measurement, and the measurement is emphatic — but it selects the closure rather than the store. The store solved a duplication problem that only exists if bundles carry keys nothing reads.

**No sharding.** Per-document bundles hold 712 to 1,475 files against a 10,000 budget. Sharding by category puts all 1,460 ACOT records in one file that a single-entry read must parse whole.

**No SQLite.** Its stated causes — retained memory, validation or filesystem overhead, database-like machinery in application code — are absent by one to three orders of magnitude.

**No host-owned job.** The threshold was missed only while the build was chunking 1.5 million localization keys per revision to feed a store that is no longer built.

## Provenance: the harness this document reports on no longer exists

This evaluation was produced by `tools/bundle-spike/`, which was **deleted when Phase 4C landed** ([STE-25](https://linear.app/unnamed-system/issue/STE-25)), alongside `tools/parser-spike/`, on which it held a Cargo path dependency. Phase 3 landed what this spike de-risked — the bundle layout, staging, atomic publication, and the pinned Revision Reader — and no recurrence obligation references its harness, so under the implementation plan's rule (a spike is deleted once the work it de-risked lands) it had nothing left to keep it alive.

The commands below therefore no longer run, and the records under `bundle-records/` can no longer be re-captured. **The measurements stay authoritative as a document**: they are what the storage decisions were made on, against the pinned build in the table below. Their `parser_spike_source`, `dds_spike_source`, and `bundle_spike_source` digests are left byte-for-byte as captured, naming source trees two of which are gone — which is what a historical record is, and why the note you are reading exists. `fixtures/bundle/` is retained.

## Reproducible record (as it was)

```bash
cargo test --manifest-path tools/bundle-spike/Cargo.toml
cargo run --release --manifest-path tools/bundle-spike/Cargo.toml --bin checks -- --capture
cargo run --release --manifest-path tools/bundle-spike/Cargo.toml --bin read -- --capture
cargo run --release --manifest-path tools/bundle-spike/Cargo.toml --bin shape -- --capture
cargo run --release --manifest-path tools/bundle-spike/Cargo.toml --bin buildtime -- --capture
cargo run --release --manifest-path tools/bundle-spike/Cargo.toml --bin verify
```

`verify` recomputed every corpus tree digest, re-hashed every compared artifact, and compared the recorded versions and source digests against the current machine, printing `ok` or `DRIFT` per record and exiting non-zero on any drift — the same contract as `tools/oracle/verify.py`.

It was shown red three times before being trusted: with `STELLARIS_WORKSHOP_ROOT` pointed elsewhere; with one byte appended to `fixtures/bundle/malformed/localisation/french/bundle_l_french.yml`, which it named by path; and with one byte appended to the harness's own `src/digest.rs`.

That third demonstration was a gate no earlier spike in this repository had. `verify` re-hashes artifacts against hashes the same run recorded, so it cannot notice that the *code* which produced them has since changed — a record can be internally consistent and describe a harness that no longer exists. Every record here pins `parser-spike`, `dds-spike`, and `bundle-spike` source tree digests, and an edit to any of them was drift. The point it was making has now been demonstrated the hard way: two of those three source trees are gone, and the provenance note above is what a reader has instead.

Corpus roots were environment-overridable exactly as the other harnesses' are. No corpus content is committed: records hold logical paths, tree digests, counts, and byte totals. The committed fixtures carry per-file digests, which is what makes the second demonstration name a file rather than report an opaque tree difference — the gap `d4-failures` found in itself.

| Pinned | Value |
| --- | --- |
| Stellaris | `Pegasus v4.4.6 (fdde)`, `v4.4.6`, mods-compat `4.4`, Steam |
| Jomini | `0.35.0`, through the parser spike's adopted `TokenReader` adapter ([ADR 0007](../adr/0007-parse-stellaris-source-through-a-wrapped-incremental-lexer.md)) |
| `image_dds` / `bcdec_rs` / `png` | `0.7.2` / `0.2.0` / `0.18.1`, through `dds-spike`'s pinned recipe ([ADR 0008](../adr/0008-decode-source-textures-through-a-pinned-conversion-recipe.md)) |
| Toolchain | `rustc 1.97.1 (8bab26f4f 2026-07-14)`, macOS, aarch64 |

The adapters were Cargo path dependencies rather than copies. A fork would have meant these bundle numbers describe a parser and a decoder no decision accepted, and a later fix to the adopted adapter would silently stop applying here.

## Declared definitions

Two terms the budget table depends on are not self-defining, and a denominator chosen after the numerator is not a budget. They were pinned before any measurement was captured.

**Canonical unsharded read-model payload.** The complete documentation model for one revision, serialized once as a single document through the canonical encoding of `docs/technical-design.md:315`. It excludes Asset Store bytes and, where the Localization Store arm is measured, shared localization chunk bytes. It is produced by a writer that exists for no other purpose, from the same in-memory model every other shape is derived from, so the ratio measures materialization overhead rather than two generators disagreeing.

**Cold.** A new process with application-owned caches cleared. It does not claim to evict the operating system's filesystem cache. Cold figures here are measured in a genuinely separate process per iteration; the child performs one operation and reports only that operation's elapsed time, so process spawn cost stays out of the number while the cache state stays cold.

**Warm.** The same process after at least one completed operation of the same kind against the same revision.

**Iteration protocol.** At least 30 measured iterations per figure after discarding five warm-up iterations. Reported as median, p95, maximum, iteration count, and machine details. Percentile rank is nearest-rank on sorted samples: with 30 iterations, p95 is the 29th. Timing distributions are recorded separately from the identity fields the drift gate compares byte for byte.

**Largest corpus.** The corpus case with the greatest generated documentation payload, identified by measurement. That is Ancient Cache of Technologies at 1,460 entries and 3.2 MiB, not Gigastructures — Gigastructures has the largest Mod Source and the largest localization, and neither is the documentation payload the budgets bind to.

**Generation scope.** The technology vertical slice, resolved through only the rows the [resolver evaluation](./resolver-evaluation.md) marks Resolved. An unresolved row refuses visibly rather than being approximated, because a bundle measurement taken over invented records measures the invention. Growth beyond that slice is reported below as a declared scaling envelope and labelled as an extrapolation wherever it appears.

## Decision budgets

The largest corpus must satisfy:

| Property | Budget | Measured (worst case) | Verdict |
| --- | ---: | ---: | --- |
| Cold revision open through validated Revision Reader | p95 ≤ 500 ms | 35.4 ms | **Met**, 14× margin |
| Warm host search, maximum result limit | p95 ≤ 100 ms | 0.22 ms | **Met**, 450× margin |
| Cold host search after revision open | p95 ≤ 250 ms | 35.0 ms | **Met**, 7.1× margin |
| Warm documentation-record read | p95 ≤ 100 ms | 0.04 ms | **Met** |
| Cold documentation-record read after revision open | p95 ≤ 250 ms | 35.6 ms | **Met**, 7.0× margin |
| Incremental retained memory, browse plus one language | ≤ 256 MiB | 0.91 MiB | **Met**, 280× margin |
| Files in one revision bundle | ≤ 10,000 | 1,475 | **Met** |
| Complete bundle validation | p95 ≤ 2 s | inside the 35.4 ms open | **Met** |
| Bundle size excluding shared stores | ≤ 2.0× unsharded and ≤ 1 GiB | 1.175× | **Met** |
| Complete build (invocation model) | p95 ≤ 3 s | 2,294 ms | **Met** |

Every budget is met by the adopted shape. Two were missed by shapes that were then rejected, and each miss selected a change rather than being waived: materializing search for all ten languages reached 2.10×, and chunking every localization key for a shared store pushed builds to 4.5 s. Both belong to arms that are no longer built.

## Method

One canonical in-memory documentation model, and every bundle shape written as a function of it. This is the load-bearing structural rule: if a browse summary and a full record were produced by two generators, a comparison between shapes would be measuring how far those generators had drifted apart, and no care in the timing harness would recover the intended number.

Five layouts and two localization placements and two search scopes were measured over six revision cases, each writing identical content.

### Controls

- **A denominator produced by the same code path.** The size ratio compares materialized views against the model they derive from, both serialized by the same function from the same value.
- **Negative controls, each shown red before its green result was used.** Three manifest injections asserting the shape of each failure separately; a garbage-collection sweep that must actually remove something; a propagation control asserting that registry-scoped impact reaches zero entries; three drift-gate demonstrations.
- **Exhaustive rather than sampled agreement.** All 1,460 ACOT entries are compared across browse summary, search summary, and full record. A sample cannot distinguish "they agree" from "they agree about the ones I looked at."
- **Cold measured in a separate process.** A fresh reader inside a warm process is not cold, and calling it cold would have made every cold figure a warm one.
- **Denominators reported beside every claim.** The reachable icon set, not every `.dds`; the entry count, not the Mod Source size.

### Two readings of retained memory, one of which failed

The design was a deep byte accounting that knows exactly what it retained and nothing about allocator behaviour, corroborated by max-RSS, which knows the opposite. The corroboration does not work: max-RSS is a high-water mark, the process peaks during the build, and loading a 0.35 MiB index afterwards cannot move it. Every RSS delta reads 0.00 MiB.

That is reported as a limitation rather than as agreement. The deep accounting stands alone. With a 280× margin it is not load-bearing, but the two-reading claim does not hold here and is not stated as though it does.

## Corpus

| Revision case | Contributors | Script | Localization | Entries | Icons resolved |
| --- | --- | ---: | ---: | ---: | ---: |
| Vanilla only | Vanilla | 4,578 | 2,318 | 698 | 674 |
| Ancient Cache of Technologies | + 1419304439 | 5,698 | 2,364 | 1,460 | 1,408 |
| Gigastructural Engineering | + 1121692237 | 6,370 | 2,727 | 998 | 966 |
| Acquisition of Technology | + 2178603631 | 5,015 | 2,558 | 862 | 807 |
| Malformed-source fixture | + `fixtures/bundle/malformed` | 4,582 | 2,320 | 707 | 674 |
| Technology-redefinition fixture | + `fixtures/oracle/target` | 4,587 | 2,318 | 701 | 674 |

Ten languages are preserved in every case. Vanilla alone holds 1,506,457 localization keys across 168 MiB of source.

The malformed case uses this spike's own fixture rather than `fixtures/parser/malformed/`. Those files are flat `.txt` at the fixture root, which is the right shape for asking what a parser does with a broken file and the wrong shape for asking what a *bundle* does: nothing in them sits on a registry path, so nothing in them can make a registry's entry set incomplete. The fault shapes are restated on registry paths, for the reason `fixtures/parser/` gives about the oracle fixtures — a fixture frozen against one spike's evidence should not silently become a dependency of another's.

## Evidence matrix

| # | Requirement | Verdict | Record |
| --- | --- | --- | --- |
| 1 | Measure generated documentation, not Mod Source sizes | **Met.** Every figure derives from a resolved, generated model; the resolver subset implements only oracle-backed rows | `b2` |
| 2 | Every denormalized view derived from one canonical model | **Met.** Structural, not asserted: the writers take a `&Documentation` and choose only file layout | `b2` |
| 3 | Bundle size against a declared denominator | **Met.** 1.12×–1.23× with the cited closure; 2.104× on the rejected all-keys arm | `b2` |
| 4 | File count and sharding comparison | **Met.** Sharding fixes nothing that needs fixing and creates a 3.1 MiB single-file read | `b2` |
| 5 | Cross-revision localization recoverable through content-addressed chunks | **Met, and superseded.** 606 MiB of 959 MiB — then measured unnecessary, because the cited closure is 1.15%–1.45% of the 959 MiB | `b2` |
| 6 | Asset Store dedup, cold conversion, warm rebuild | **Met.** 2.91×; 1,832 unique blobs at 10.7 MiB against 31.3 MiB referenced | `b1` |
| 7 | End-to-end and per-phase build time | **Met.** p95 1.8–2.3 s against a 3 s threshold | `b1` |
| 8 | Cold and warm open, search, and record latency | **Met.** Every budget passes with 7× margin or better | `b3` |
| 9 | Retained index memory | **Met, by one reading.** 0.91 MiB against 256 MiB; the corroborating reading is uninformative | `b3` |
| 10 | Manifest detects missing, changed, and unexpected entries | **Met.** Three separate injections, each asserting its own failure shape | `b4` |
| 11 | Reader cannot address staging or unreferenced bundles | **Met.** By construction — there is one constructor and it takes a published identifier | `b4` |
| 12 | Views agree on identity, category, provenance, localization keys | **Met.** 1,460 entries, zero disagreements | `b4` |
| 13 | Language switching and fallback without rebuilding | **Met.** All three fallback steps exercised against one immutable revision, and the closure proved observationally equal to preserving every key | `b4` |
| 14 | Malformed source publishes Incomplete Documentation | **Met.** Valid bundle, registry incomplete, zero entries carrying the file-fault issue | `b4` |
| 15 | Asset keys stable, and change on source or recipe change | **Met.** Both directions, and the required set is exact | `b4` |
| 16 | Garbage collection preserves live blobs and honours the grace period | **Met**, with the sweep's own negative control | `b4` |
| 17 | Determinism | **Met.** A rebuild through an entirely different asset path yields the same Revision identifier | `b4` |
| 18 | Companion response size and time | **Partial.** Host-side serialization only; no listener is stood up | — |
| 19 | Cleanup of abandoned staging and unreferenced complete bundles | **Partial.** Unreferenced bundles are proved unreachable; timed sweep behaviour is not measured | `b4` |

Requirements 18 and 19 are partial and disclosed rather than rounded up.

## Findings

### The documentation cites one percent of the localization it was going to carry

A Gigastructures revision serialized with every key is 191.1 MiB. Its documentation is 2.4 MiB. The other 188.7 MiB is ten languages of preserved localization — 1,700,609 keys.

The documentation cites 1,996 of them by name, and following the closure of their static `$key$` references adds 366 more.

| Case | Entries | Cited keys | After closure | Bytes, all 10 languages | Share of preserved |
| --- | ---: | ---: | ---: | ---: | ---: |
| Vanilla | 698 | 1,396 | 1,660 | 1.74 MiB | 1.15% |
| ACOT | 1,460 | 2,920 | 3,172 | 1.95 MiB | 1.27% |
| Gigastructures | 998 | 1,996 | 2,362 | 2.59 MiB | 1.45% |
| Acquisition of Technology | 862 | 1,724 | 2,007 | 2.26 MiB | 1.45% |

The closure is shallow and terminates: depth four, adding 264 to 366 keys. It is taken across every language at once rather than per language, because a reference present in one translation may be absent from another, and a per-language closure would preserve a key for French and drop it for German — losing text on exactly the language switch that preserving every language exists to protect.

This is the measurement the spike should have taken first. Everything before it — the shared store, content-defined chunking, the chunking-dominated build, the size budget failing on search material — was arithmetic about 150 MiB per revision that no reader ever reads.

### Localization storage was a question about the wrong quantity

Two chunking schemes were measured before the closure was, and both are now history rather than design. They are kept here because the comparison is what makes the closure's case, not because either is adopted.

| Scheme | Carried | Unique | Chunks | Ratio | Recovered |
| --- | ---: | ---: | ---: | ---: | ---: |
| One chunk per language table | 959.2 MiB | 508.2 MiB | 31 | 1.89× | 451.0 MiB |
| Content-defined boundaries | 959.2 MiB | 352.7 MiB | 5,553 | 2.72× | 606.4 MiB |

Content-defined boundaries are genuinely better than the obvious scheme, by 155 MiB. Whole-language chunking dedupes only the languages a mod never touches — Japanese and Korean here, which Gigastructures leaves at exactly 149,217 keys because it ships no translation for either. One added English key turns a 15 MiB table into a new blob.

None of that matters once the quantity being deduplicated is 2 MiB. A 2.72× deduplication ratio on 959 MiB is a smaller saving than not storing the 959 MiB, and it costs a store, a chunking module, chunk-key manifests, cross-store garbage collection, and roughly half of every build.

### Static references are real, and a naive cited-key implementation would drop text

Across 29,200 expansions of documented entry names and descriptions, 2,915 static references are substituted — roughly one documented text in ten embeds one.

An implementation that preserved only the keys the documentation names, without following references, would render those with a raw `$key$` visible in the middle of a sentence. The closure is not defensive machinery; it is the difference between correct text and visibly broken text on about a tenth of pages.

Two checks stand behind it, and one of them had to be rewritten before it could.

`closure/reference-closed` walks every preserved value, follows every static reference, and asserts that any name resolving in the unpruned tables also resolves in the pruned ones. It is what distinguishes a closure that reached its fixpoint from one the depth bound truncated.

`closure/observationally-equal` compares the **expanded** display text of every entry name and description, in every language, against a model that preserved every key. Its first version compared raw values instead, and the negative control caught it: with the closure truncated to zero depth, it reported zero differences while 277 references dangled. Entry name and description keys are closure *seeds* and survive any truncation, so that version could not fail for the reason it existed. Comparing expanded text makes it discriminate — 2,576 of 29,200 expansions differ under the same injection.

### Search material is the second-largest artifact, and requirements name two languages

Search is the one artifact that materializes localized text, so it is the only one that multiplies by the language count while only one language is ever active.

| Case | Records | Search, ten languages | Search, selected + English |
| --- | ---: | ---: | ---: |
| Vanilla | 1.54 MiB | 1.37 MiB | 0.28 MiB |
| ACOT | 3.14 MiB | 2.90 MiB | 0.58 MiB |
| Gigastructures | 2.34 MiB | 2.06 MiB | 0.42 MiB |

**Two languages, not one.** Product requirement 28 keeps English names searchable in any configured language so guides and community discussion stay usable, and requirement 21 lists selected-language names and English names as separate index inputs. An earlier revision of this harness built one table and called it the active language, which was correct only when the selected language happened to be English. That error is recorded rather than quietly fixed, because it was on its way into an ADR.

With cited-closure localization the choice is no longer forced. Materializing all ten reaches 1.47× to 1.67×, comfortably inside the 2.0× budget; selected-plus-English reaches 1.12× to 1.23×. Both pass. Selected-plus-English is adopted because it is smaller and matches the requirements exactly, but a later decision to index every language would not reopen the format question.

It was forced under the rejected shared-store arm, where all ten reached 2.03× to 2.10× and missed. That is the miss the earlier capture recorded, and it belongs to a shape that is no longer built.

### Sharding solves a problem this corpus does not have, and creates one it did not have

Per-document bundles hold 712 to 1,475 files against a 10,000 budget — one seventh of the limit on the largest case. The design reserved sharding for "an excessive file count".

Sharding also makes single-entry reads worse. `by_category` puts all 1,460 ACOT technology records in one file, so reading one entry parses the whole category, for a 2% size saving.

The digest-based prefix is worth recording as a detail: an identifier-prefix scheme would have been useless here, because every Stellaris technology identifier begins `tech_`. The shard test asserts that 200 synthetic identifiers reach all 16 buckets, which a prefix scheme could not do.

### What a build costs once it stops chunking

p95 complete builds are 1,817 to 2,294 ms against a 3,000 ms threshold, so an awaited asynchronous Tauri command is preferred and navigation-independent execution is not required.

| Phase | Share of a cold build |
| --- | ---: |
| Fingerprint (first pass) | 24.2% – 31.2% |
| Final live-source re-verification | 25.3% – 30.8% |
| Resolve | 22.5% – 30.7% |
| Asset materialization | 1.5% – 18.8% |
| Write, validate, publish | 4.7% – 8.5% |
| Generate | 0.5% |
| Localization chunking | 0% — not performed |

The correctness-first double fingerprint is now the dominant cost at 50% to 62% combined. `docs/technical-design.md:414` requires it — the second pass is what proves the source did not change during analysis — and this is what it costs. It hashes the complete logical snapshot including all 168 MiB of localization source, which the revision no longer carries but whose bytes still participate in freshness identity.

That is the honest remaining lever, and it is a correctness-sensitive one. Nothing here suggests weakening it.

### JSON was never the constraint

Every read-side budget passes by between 7× and 2,500×. A cold open that validates 1,475 required entries by re-hashing all of them takes 35 ms; the budget for validation alone is 2 seconds.

This is worth stating plainly because the spike's premise was that JSON might not be suitable. On the read path it is not merely suitable, it is nowhere near its limits. Every budget that failed at any point in this spike failed on *what was being written* — ten languages of search material, or 1.5 million localization keys being chunked — and none had a cause the rule assigns to SQLite: not retained memory, not validation or filesystem overhead, not database-like indexing machinery in application code.

### Asset deduplication is 2.91×, and the reachable set is what makes it meaningful

1,832 unique blobs holding 10.7 MiB serve 31.3 MiB of per-revision references across six revisions. Four of those revisions reference the same 674 Vanilla technology icons, and each stores none of them again.

The denominator matters here, as it did in the DDS spike. This measures the 674 to 1,408 icons the generated documentation actually references, not the 33,145 `.dds` files the corpora contain. A dedup ratio over the second number would be a fact about the filesystem.

Icons resolve by the path convention `gfx/interface/icons/technologies/<identifier>.dds` rather than through a sprite lookup, which is a disclosed narrowing: whether the game also accepts a sprite indirection for technologies is unexercised here. Between 3.4% and 6.4% of technologies per case have no icon at that path and receive a deterministic placeholder plus an entry-scoped issue.

### Two harness defects that would have shipped as findings

Both were found by running the thing rather than by reading it, and both are recorded because they are the failure mode this spike is about.

**Pretty-printed JSON charged the denominator for nesting depth.** The first capture reported an in-bundle ratio of 0.947 — a bundle smaller than the model it contains, which is not a thing that can be true. Indented JSON pays two spaces per nesting level, and the single-document payload nests every record several levels deeper than a per-document file does. The comparison was measuring indentation. Bundle payloads are now compact and only the manifest is pretty-printed, which is all `docs/technical-design.md:632` ever asked for.

**A check that could not fail.** `closure/observationally-equal` compared raw entry name and description values, which are closure seeds and survive any truncation. Truncating the closure to zero depth left it reporting zero differences while 277 references dangled. It is fixed to compare expanded display text, and only the negative control revealed the difference — the diagnostic in `AGENTS.md` about a gate that has always been green, found in this spike's own suite.

**Warm builds measured slower than cold ones, twice, for two different reasons.** The first was that the loop republished onto its own destination, so every timed iteration silently included deleting the previous bundle's ~700 files — retention cleanup, which the design defers until every handle closes, charged to publication. The second survived that fix: the phase sum omitted localization chunking entirely, so the cold column was not measuring the same thing as the warm column beside it. A per-phase breakdown that does not sum to the operation is not a breakdown.

### A bundle can be written, validated, and published, and still be unreadable

`skip_serializing_if` without `default` produced bundles whose first entry with no `categories` could not be deserialized. The bundle was written, its manifest validated every required entry by hash, and it was published — and then the first read panicked.

Manifest validation cannot catch this. It hashes bytes; it does not parse them. A bundle format whose validation proves integrity but not readability has a gap exactly the width of its own serde attributes, and the only thing that closes it is a round-trip test over a value with every optional field absent. That test now exists and was shown to fail before the fix.

### The drift gate every earlier spike here is missing

`verify` re-hashes a record's artifacts against hashes that same run recorded. It cannot notice that the harness which produced them has changed, so a record can be internally consistent and describe code that no longer exists. Every record here pins its own crate's source tree digest alongside the two adapter crates', and an edit to any of the three is reported as drift.

This is a small change with a specific motivation: the two adapters were path dependencies, so a local edit to the parser spike's lexer would change what was measured while every recorded version number stayed still.

## Rejected shortcuts

- **Measuring Mod Source sizes.** A revision's cost is a fact about generated documentation, several transformations away from the bytes on disk. Gigastructures has the largest Mod Source and is not the largest documentation payload.
- **Generating documentation for registries whose Resolution Profile rows are unresolved.** It would have produced larger, more impressive bundles measuring content this harness invented. The technology slice plus a labelled envelope is a smaller claim that is true.
- **Letting each bundle writer produce its own content.** Then a size comparison measures generator divergence. One model, functions of it, writers that choose only layout.
- **Calling a fresh reader in a warm process "cold".** Cold figures are measured in a separate process per iteration, with spawn cost excluded from the number rather than from the cache state.
- **Reporting max-RSS as corroboration when it corroborates nothing.** It is a high-water mark and the process had already peaked; every delta reads zero. Reported as a failed control, not as agreement.
- **Waiving the size budget at 2.05× because 2.0 was "about right".** The spike's own rule forbids it, and following the rule found the actual cause — per-language search material — which sharding would not have addressed.
- **Reaching for SQLite because a budget failed.** The rule assigns SQLite to specific causes. Neither miss had one, and both were fixed by changing what is written.
- **Trusting that a validated bundle is a readable one.** It hashes bytes. It does not parse them.

## Declared scaling envelope

The measured slice is technology. Growth beyond it is an extrapolation, stated as one.

At 2.2 KiB of documentation per entry measured across the six cases, the parser census's whole-corpus definition counts give an envelope for a hypothetical every-registry bundle:

| Corpus | Definitions | Envelope, documentation only |
| --- | ---: | ---: |
| Vanilla | 58,398 | ~126 MiB |
| Gigastructures + Vanilla | 100,540 | ~216 MiB |
| ACOT + Vanilla | 74,171 | ~160 MiB |

This is not evidence that a larger bundle meets these budgets. It multiplies a per-entry cost measured on one content type by definition counts from content types with different field densities, and it says nothing about latency or retained memory at that scale. Its purpose is to establish that the current 4 MiB result is not the eventual one, and that the file-count budget — around 100,000 files at one file per document — is the first thing an every-registry bundle would break.

The format accepted here is accepted for the measured slice plus this stated envelope. Adding a content type is a new measurement, not an inherited result.

## Completion model

### Evidence collection

Complete. Four records, every number traceable to one of them, the drift gate green across all four and demonstrated red three times.

### Format acceptance

Materialized JSON with one file per document, carrying the cited-key closure, meets every declared budget on the largest corpus without loading complete documentation into memory. Sharded JSON, SQLite, a shared Localization Store, and a host-owned build job are all measured out rather than left unmentioned.

### Known limitations, carried forward

- **The closure is measured on the keys this generator cites** — entry name and description. Production documentation will cite more: category names, modifier text, requirement descriptions. Even a tenfold larger seed set lands near 20 MiB across ten languages, still ~88% below preserving everything, and it should grow sublinearly because entries share category and modifier keys. This is the single assumption most worth re-measuring when the generator deepens.
- **The closure follows `$key$` static references only.** `[Concept]` runtime tokens and `£icon£` markers stay raw by ADR 0004 and are correctly excluded, but that is a stated assumption rather than a tested one. A reference form neither check recognizes would be invisible to both.
- The Companion response measurement serializes the response DTO and times the host side. No HTTP listener is stood up, so network transfer, framing, and connection behaviour are unmeasured.
- Cleanup of abandoned staging directories and unreferenced complete bundles is proved unreachable by a reader; its timed sweep behaviour under a real retention policy is not measured.
- The retained-memory corroboration failed as described. One reading, not two.
- Search normalization implements case folding and whitespace collapse but not NFKC, because Rust's standard library has no normalizer and pinning one for a byte-and-latency measurement was not worth another drift-checked dependency.
- Inline-script expansion happens at the parsed-item level rather than over raw bytes. It reproduces substitution and nesting; it does not reproduce a case where an expansion changes how surrounding text lexes.
- Technology icons resolve by path convention. A sprite indirection, if the game accepts one for technologies, is unexercised.
- Determinism is measured across a rebuild in one process on one machine, through a different asset code path. Randomized worker schedules and cross-machine reproducibility are not measured.
- `verify` compares `rustc --version` byte for byte and the repository pins no toolchain, so every record here goes red on the next Rust update.

### Current standing

| Dimension | Standing |
| --- | --- |
| Read latency | Complete; every budget met with 7× margin or better |
| Retained memory | Met by the deep accounting; the corroborating reading failed |
| Bundle size | Met at 1.12×–1.23×; all-language search also passes at 1.47×–1.67× |
| File count | Met; sharding measured out |
| Localization | Cited-key closure, in bundle, every language; no shared store |
| Build duration | Met at p95 1.8–2.3 s; awaited Tauri command |
| Correctness | 22 checks, every gate shown red before use |
| Reproducibility | Complete; gate shown red three times, including on the harness's own source |

## Captured records

| Run | Answers | Artifacts |
| --- | --- | --- |
| `b1-build` | Where build time goes, and what the Asset Store holds and deduplicates | `phases.json`, `assets.json`, `asset-store.json`, `timings.json`, `summary.txt` |
| `b2-shape` | Bytes and file count per layout against the declared denominator; the cited-key closure against preserved localization; the rejected chunking arms | `cases.json`, `shapes.json`, `localization-store.json`, `summary.txt` |
| `b3-read` | Cold and warm open, search, and record latency; retained index memory | `memory.json`, `timings.json`, `summary.txt` |
| `b4-checks` | The 22 correctness checks and their negative controls, including closure completeness | `checks.json`, `summary.txt` |

Each manifest holds the run's purpose verbatim from its binary, the pinned environment including three source tree digests, every corpus identified by tree digest, and every artifact by hash. The manifest is written last and hashes what is already on disk, so it can never name an artifact that was not produced.

`b1-build` and `b3-read` additionally declare `uncompared_artifacts`: their timing distributions, named with the reason they are excluded from byte comparison. `d3-recipe` had to delete two wall-clock fields to stay reproducible; this spike cannot, because the timings are the evidence, so the exclusion is structural and visible rather than silent. Their existence is still required — a missing timings file is drift.

The harness lived in `tools/bundle-spike/`, outside `src-tauri`'s workspace, and took Cargo path dependencies on `tools/parser-spike/` and `tools/dds-spike/`, both of which were their own workspace roots, so nothing entered the application's dependency graph until the production modules were implemented. That path dependency on the parser spike is why the two harnesses were deleted together; see the provenance note above.
