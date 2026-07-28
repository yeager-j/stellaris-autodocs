# Parser conformance records

Captured runs of the whole-corpus parser conformance harness
(`src-tauri/src/analysis/parser/conformance/`), which reads every enumerated script file of an
installed Stellaris and ACOT through the production adapter and checks the result three ways:
every derived byte range is re-sliced from the source it claims to cover, the parsed-corpus
digest is recomputed under a reversed fold and under serial execution, and an independent
second reading through Jomini's `TextTape` is compared against the adapter's over the files
both readings accept.

These are **production** records. The spike records under `docs/spikes/*-records/` are the
earlier evidence that led to the decision (`docs/adr/0007`); they are history and are not
maintained.

## Capturing and checking

From `src-tauri/`:

```bash
cargo test --features test-support corpus_conformance -- --ignored --nocapture
```

That checks against the committed record and writes nothing. Adding
`PARSER_CONFORMANCE_CAPTURE=1` writes the record instead, and accepts whatever drift the run
found — capture is the deliberate act of saying the new state is the right one. The
conformance checks themselves are never waived, so a failing run is never recorded.

Corpus roots come from `STELLARIS_INSTALL_ROOT` and `STELLARIS_WORKSHOP_ROOT`, defaulting to
the macOS Steam locations. A missing root fails the run rather than skipping it.

**Re-run on a Stellaris build change, a Jomini upgrade, or a dialect-lexer edit** — the same
standard `docs/adr/0008` holds for a texture-decoder upgrade.

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
| `corpora[].outcome` | What the run *got* from it: the `parsed_corpus_digest`, and how the cross-check resolved — compared, agreed, diverged, tape-rejected, adapter-recovered, plus range faults. |
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
| `divergences.txt` | Every structural disagreement, one per line, each pinned and traced in `conformance/expected.rs`. |
| `tape-rejections.txt` | Files the second reading refused, with Jomini's message. Not failures: the tape rejects real dialect the adapter handles. |
| `recoveries.txt` | Files the adapter recovered from. Excluded from the structural comparison — a repaired file is expected to differ from a reshaped one — and listed so a wrapper defect appearing as a spurious fault cannot hide in the excluded set. |

Every list prints its total before its lines, so a truncated list cannot read as a complete
one. The lists are compared against the record as well, on two counts: they say *which*
findings there were where `outcome` says how many, and they are the only place a change in the
**second** reading surfaces — `parsed_corpus_digest` is the production adapter's reading, so a
Cargo.lock bump from jomini `0.35.0` to `0.35.1` leaves the declared `0.35` still and shows up
in `tape-rejections.txt` or nowhere.

**No corpus content is copied into this repository.** Logical paths and digests are what a
licensed local installation needs to reproduce a run, and they are all a record carries for a
corpus outside the repo.

## Extending the format

Phase 4M ([STE-33](https://linear.app/unnamed-system/issue/STE-33)) adds a parse-**and-resolve**
run. It extends this format rather than replacing it: a new record directory beside
`c1-parser-conformance`, the same manifest keys, the same corpus identities, with the
resolver's outcome summary and per-cell visible-failure counts as additional artifacts.
Keeping the corpus identity and environment blocks identical is what lets one drift
comparison serve both runs.
