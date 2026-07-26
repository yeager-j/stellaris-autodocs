# 0009 — Store revisions as self-contained per-document JSON

Status: Accepted

Date: 2026-07-26

## Context

`docs/technical-design.md` provisionally described a Documentation Revision as an immutable
directory of build-time denormalized JSON, holding it open that individual documents might be
sharded if one file per document produced an excessive file count, and that SQLite might
replace the format if a real-corpus measurement showed file count, bundle size, validation
time, index memory, or cold-read latency to be unsuitable. It separately pre-authorized a
shared content-addressed Localization Store *if* preserved all-language localization proved to
dominate cross-revision duplication.

None of that had been measured. The Revision Reader boundary exists so the choice could be
made late, but `revisions`, `search`, `assets`, and the publication protocol all have to be
built against one answer.

The [revision bundle evaluation](../spikes/revision-bundle-evaluation.md) declared nine budgets
and a build-duration threshold before collecting the deciding measurements, then built six
revision cases end to end over Vanilla Content, Ancient Cache of Technologies, Gigastructural
Engineering, Acquisition of Technology, and two golden fixtures.

## Decision

A Documentation Revision is an immutable, **self-contained** directory of materialized JSON
with one file per document.

**A revision preserves the localization its documentation cites**, plus the closure of that
set's static references, in every available language. Not the complete tables, and not a live
read against Mod Source.

**Build-time search material covers the selected language and English**, which product
requirements 21 and 28 name as separate index inputs. If the user later selects another
available language, the search module derives that language's index in memory from localization
and entries already present in the immutable revision. It does not reparse source, mutate the
bundle, or rebuild Player Documentation.

**A build is an awaited asynchronous Tauri command.**

Sharding, SQLite, a shared Localization Store, and a host-owned build job are all rejected.

Bundle payloads are compact JSON. Only the manifest is human-readable, which is all the design
ever asked for.

## Why

The documentation cites between 1.15% and 1.45% of the localization a revision was going to
carry. Preserving the cited set plus its reference closure costs 1.74 to 2.59 MiB across all
ten languages, against 151 to 178 MiB for the complete tables.

That single measurement resolves four separate questions at once. Every size budget passes
with the closure in the bundle. The shared store becomes storage for keys no reader reads. The
chunking phase that store required — 45% to 53% of every build, 1.5 million digests per
revision — stops existing, which brings p95 builds from 3.7–4.5 s down to 1.8–2.3 s and back
inside the three-second threshold. And the revision stays self-contained, so nothing about
reads depends on Mod Source still being present.

Following the reference closure is not optional. Across 29,200 expansions of documented entry
names and descriptions, 2,915 static `$key$` references are substituted — roughly one
documented text in ten. Preserving only the keys the documentation names by hand would render
those with a raw placeholder mid-sentence.

Resolving localization from live source at read time was considered and rejected. It saves the
remaining 2 MiB and costs the property the rest of the design rests on: a revision's rendered
content would stop being a function of its identity, reads would depend on Mod Source being
present and readable, a stale revision would render new strings against old documentation, and
Documentation Export would become impossible without the game installed.

Sharding fixes nothing this corpus has. Per-document bundles hold 712 to 1,475 files against a
10,000 budget, and sharding by category makes a single-entry read parse an entire category file
for a 2% size saving.

SQLite is not reached because no budget failure had a cause the rule assigns to it. Retained
memory is 0.91 MiB against 256 MiB. A cold open that re-hashes all 1,475 required entries takes
35 ms against a 2-second validation budget. Cold search and record reads are under 36 ms
against 250 ms. Every failure at any point in the spike was about what was being written.

## Consequences

- `revisions` owns per-document JSON layout, the manifest, and the two-commit publication
  protocol. Replacing that layout later remains a new Revision Reader implementation and
  nothing above it.
- `analysis` gains ownership of the cited-key closure: which keys the documentation references,
  the transitive closure of their static references taken across all languages at once, and the
  pruned localization that enters the Revision Candidate. `localization` keeps its semantic
  authority over selection, fallback, tokenization, and projection.
- The closure is a correctness surface, not an optimization. It requires a check that the
  fixpoint terminated rather than hit its bound, and a check that expanded display text is
  identical to a model preserving every key. Both must be shown red before being trusted; the
  second was written wrongly the first time and passed a truncated closure.
- No Localization Store, no chunk-key manifest entries, and no cross-store garbage collection.
  Manifests still carry required Asset Store keys.
- Search indexes for languages not selected when the revision was built are disposable runtime
  derivations. The same search module owns build-time and on-demand construction so matching and
  normalization do not acquire a second authority.
- Build invocation is an awaited asynchronous Tauri command. The single-build-lease rule is
  unchanged and independent of this.
- The accepted format covers the technology vertical slice plus the evaluation's declared
  scaling envelope. The closure is measured against the keys this generator cites; a deeper
  generator cites more, and that is the assumption most worth re-measuring.

## Alternatives rejected

**Preserving every key.** 151–178 MiB per revision to store what no reader reads.

**A shared content-addressed Localization Store.** Genuinely effective at what it did —
content-defined chunking recovered 606 MiB of 959 MiB against 451 MiB for whole-language
chunks — and unnecessary once the quantity being deduplicated is 2 MiB. It cost a module,
chunk-key manifests, cross-store garbage collection, and half of every build.

**Resolving localization at read time.** Saves 2 MiB, costs the self-contained immutable
artifact.

**Preserving only the cited keys without the closure.** Breaks visible text on roughly one
documented page in ten.

**Sharded JSON.** Measured on three configurations against a file count one seventh of budget.

**SQLite.** Measured out; its stated causes are absent by one to three orders of magnitude.

**A host-owned build job.** Required only while the build was chunking localization for a store
that is no longer built.
