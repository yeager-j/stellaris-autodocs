# Corpus conformance records

Captured runs of the two whole-corpus conformance harnesses, which read an installed
Stellaris and ACOT and pin what the production code makes of them:

- **`c1-parser-conformance`** (`src-tauri/src/analysis/parser/conformance/`) reads every
  enumerated script file through the production adapter and checks the result three ways:
  every derived byte range is re-sliced from the source it claims to cover, the parsed-corpus
  digest is recomputed under a reversed fold and under serial execution, and an independent
  second reading through Jomini's `TextTape` is compared against the adapter's over the files
  both readings accept.
- **`c2-parse-and-resolve`** (`src-tauri/src/analysis/resolver/conformance.rs`, Phase 4M)
  resolves every declared Resolution Profile row, and the localization file stream, over the
  same two corpora through the production resolver. Each row's outcome is recorded on equal
  terms: definition and typed-fact counts for a row that resolves, and the typed refusal —
  the policy cell it stopped at — for a row that does not. A hit on an unresolved cell is a
  recorded visible failure, never an error and never a fallback.

These are **production** records, and checking against them is a required acceptance
activity from Phase 4 onward — a failure after a game or mod update is signal, not noise.
The spike records under `docs/spikes/*-records/` are the earlier evidence that led to the
decision (`docs/adr/0007`); they are history and are not maintained.

## Capturing and checking

From `src-tauri/`:

```bash
cargo test --features test-support corpus_conformance -- --ignored --nocapture
cargo test --features test-support parse_and_resolve_conformance -- --ignored --nocapture
```

Each checks against its committed record and writes nothing. Adding
`PARSER_CONFORMANCE_CAPTURE=1` (for `c1`) or `PARSE_AND_RESOLVE_CAPTURE=1` (for `c2`) writes
the record instead, and accepts whatever drift the run found — capture is the deliberate act
of saying the new state is the right one. The conformance checks themselves are never
waived, so a failing run is never recorded.

Corpus roots come from `STELLARIS_INSTALL_ROOT` and `STELLARIS_WORKSHOP_ROOT`, defaulting to
the macOS Steam locations. A missing root fails the run rather than skipping it.

**Re-run both on a Stellaris build change, a Jomini upgrade, or a dialect-lexer edit** — the
same standard `docs/adr/0008` holds for a texture-decoder upgrade. Re-run `c2` additionally
on an ACOT update or any Resolution Profile change.

## What a record contains

`manifest.json` is written last, hashing the artifacts already on disk, so it can never name
a file that was not produced.

| Key | Meaning |
| --- | --- |
| `run` | The record directory's own name. |
| `purpose` | What the run measured, in enough detail to judge whether the numbers answer it. |
| `environment` | The installed Stellaris build (from `launcher-settings.json`, never from `game.log`), the Jomini requirement the harness linked against, `rustc`, os, arch. |
| `corpora[]` | What each corpus *is*: `id`, `title`, the number of script files parsed, their total bytes, and the production `/v3` source `fingerprint`. |
| `corpora[].files` | Per-file digests — **only** for corpora inside this repository. A licensed local installation is identified by its fingerprint and counts; listing a shipped product's files here would add nothing verifiable. |
| `corpora[].outcome` | `c1` only — what the run *got* from that corpus: the `parsed_corpus_digest`, and how the cross-check resolved — compared, agreed, diverged, tape-rejected, adapter-recovered, plus range faults. `c2` records identity-only corpora, because resolution is an answer about the *pair*. |
| `resolution` | `c2` only — the Resolution Profile version, one row per declared registry (`resolved` with a `semantic_digest` over the row's complete resolved output plus definition, typed-fact, and visible-failure counts, or `refused` with the policy cell and typed reason), and the localization file stream's outcome with its own `stream_digest`. The digests are the identity and the counts are diagnostics: a duplicate-winner swap or a moved provenance site can leave every count unchanged, exactly the gap `parsed_corpus_digest` closes for `c1`. |
| `artifacts` | Artifact name to SHA-256 of the bytes on disk. |
| `warnings` | Anything the run wants a reader to know but did not fail on, such as a corpus that established incomplete. |

The `outcome` block is compared, not merely reported, and that is the point of recording it.
The fingerprint answers "did the source move" and the environment answers "did the tools
move"; neither answers **"did the parser start reading the same bytes differently"** — which
is exactly what the third recurrence trigger, a dialect-lexer edit, causes. A change in
`ScalarKind`, in a derived range that still re-slices correctly, or in evidence quality is
invisible to the structural cross-check by design. `parsed_corpus_digest` covers all of it.

Artifacts:

| File | Contents |
| --- | --- |
| `divergences.txt` (`c1`) | Every structural disagreement, one per line, each pinned and traced in `conformance/expected.rs`. |
| `tape-rejections.txt` (`c1`) | Files the second reading refused, with Jomini's message. Not failures: the tape rejects real dialect the adapter handles. |
| `recoveries.txt` (`c1`) | Files the adapter recovered from. Excluded from the structural comparison — a repaired file is expected to differ from a reshaped one — and listed so a wrapper defect appearing as a spurious fault cannot hide in the excluded set. |
| `resolution.txt` (`c2`) | The human-readable listing of every row's outcome: each typed-fact and visible-failure count for a resolved row, and the full refusal message for a refusing one. |

Every list prints its total before its lines, so a truncated list cannot read as a complete
one. The lists are compared against the record as well, on two counts: they say *which*
findings there were where `outcome` says how many, and they are the only place a change in the
**second** reading surfaces — `parsed_corpus_digest` is the production adapter's reading, so a
Cargo.lock bump from jomini `0.35.0` to `0.35.1` leaves the declared `0.35` still and shows up
in `tape-rejections.txt` or nowhere.

**No corpus content is copied into this repository.** Logical paths and digests are what a
licensed local installation needs to reproduce a run, and they are all a record carries for a
corpus outside the repo.

## One format, two runs

Phase 4M ([STE-33](https://linear.app/unnamed-system/issue/STE-33)) added `c2` by extending
`c1`'s format rather than inventing a second one: the same manifest keys, the same corpus
identities, with the resolver's outcome summary as the optional `resolution` block and its
listing as an additional artifact. The shared record shape and drift comparison live in
`src-tauri/src/analysis/conformance.rs`; keeping the corpus identity and environment blocks
identical is what lets one drift vocabulary serve both runs. The resolution drift
comparisons carry CI-runnable negative controls there, and each gate was additionally proven
red once against a modified corpus root — the observed failures are documented in the two
harnesses' module comments.
