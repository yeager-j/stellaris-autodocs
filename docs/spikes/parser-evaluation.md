# Parser evaluation

Status: Complete against Stellaris `Pegasus v4.4.6` and Jomini `0.35.0`. Jomini is adopted, wrapped, and not used as it stands.

The hypothesis held, but not in the way the spike anticipated. Jomini can provide the syntax foundation, and the app-owned parser interface it is wrapped behind is not a matter of hygiene: it is where the corpus is actually made parseable. Two of the three Jomini APIs were measured against 7,938 real files, and the one this repository had assumed — `TextTape` — turns out to be the weaker of them for this application.

Nothing here is a claim about Jomini's quality. It is a mature, fuzz-tested parser for Paradox save games, and it is faster than the wrapper built on top of it. The gap is between what a save-game parser needs and what a documentation tool that must cite its sources needs.

## Decision

Adopt Jomini, consumed through its `TokenReader` incremental lexer rather than its `TextTape`, behind the application-owned parsed representation the technical design already requires. The adapter adds three things the reader does not do: source spans, resynchronization after a syntax fault, and about 55 lines of Stellaris-specific lexing for constructs Jomini does not recognize.

This is the spike's second listed outcome — extend or wrap to close specific, bounded gaps — and the gaps are enumerated in [Findings](#findings) rather than described in general.

The `TextTape` path is retained in the harness as a cross-check, not as a fallback. It is what caught every defect in the wrapper, and it is what would catch the next one.

## Reproducible record

```bash
cargo test --manifest-path tools/parser-spike/Cargo.toml
cargo run --release --manifest-path tools/parser-spike/Cargo.toml --bin coverage -- --capture
cargo run --release --manifest-path tools/parser-spike/Cargo.toml --bin verify
```

`verify` recomputes every corpus tree digest and compares the recorded Stellaris build, Jomini version, and rustc version against the current machine, printing `ok` or `DRIFT` per record and exiting non-zero on any drift — the same contract as `tools/oracle/verify.py`. It was shown red before being trusted: it failed on `p1-coverage` on its first real run, correctly, because a fixture comment had been edited after capture, and it fails again on demand with `STELLARIS_WORKSHOP_ROOT` pointed elsewhere.

Corpus roots are environment-overridable exactly as the oracle harness's are. No corpus content is committed: records hold tree digests, file counts, and byte totals, which is what a licensed local installation needs to reproduce a run.

| Pinned | Value |
| --- | --- |
| Stellaris | `Pegasus v4.4.6 (fdde)`, `v4.4.6`, mods-compat `4.4`, Steam |
| Jomini | `0.35.0`, default features off |
| Toolchain | `rustc 1.97.1 (8bab26f4f 2026-07-14)`, macOS, aarch64 |

## Method

Two adapters were built over one application-owned model, and the mature one was used to check the other.

- **Path A — tape.** `TextTape::from_slice`, walking `tape.tokens()` directly rather than the `ObjectReader` DOM, whose `field_groups` collapses repeated keys into groups. That normalization would have destroyed the duplicate ordering the resolver depends on before the resolver ever saw it.
- **Path B — lexer.** `TokenReader` plus `position()`, producing byte ranges on every node and resynchronizing past syntax faults.

On every file both adapters accept, Path B's model must be structurally identical to Path A's once spans are excluded. Spans are excluded from that comparison deliberately: Path A has almost none to contribute, and folding them in would have made every file mismatch for the uninteresting reason.

### Controls

- **Cross-check.** The divergence count and its shape is the measurement, not a sanity check. It is what decides whether a hand-built wrapper is maintainable or is quietly a second parser, and it found every wrapper defect this spike fixed.
- **Round trip.** Every span is re-sliced from the original bytes and compared to the text it claims to cover, on every node of every file. A derivation right about 99% of a corpus is a bug, and nothing short of exhaustive checking separates the two.
- **Negative controls.** Each gate was shown failing before its green result was used. A deliberately shifted span makes the round-trip checker report exactly one fault with the expected and found text; the boundary test that forbids `jomini` in the model requires the two adapter modules to still import it; the drift gate exits 1 with a moved corpus root.
- **Positive control.** In the blast-radius run, truncating the *last* definition of a file is a fault after which recovery can reach nothing. The lexer must lose exactly one definition there and no more.
- **Denominators.** Coverage claims are reported beside a census of what the corpus actually contains. "Every file parsed" says nothing if the corpus never exercised a comparison operator or a mixed container.

### Why two adapters rather than one

A single adapter would have had nothing to be wrong against. Neither the game nor Jomini reports what a *correct* parse of a Stellaris file is, and the corpus is far too large to read. Two independent readings of the same bytes make disagreement visible, and every disagreement is either a defect or a finding.

Eight defects in the wrapper were fixed during this spike. Five were found by the corpus and by nothing else: a missing `;` in the trivia rule, the `]` of a runtime scope token read as a conditional close, conditional blocks not recognized at all, an elided `=` between a key and a block, and scope tokens split at their brackets. The other three — the span derivation, a broken container hoisting its children into its parent, and a fault at end of input attributed to the wrong place — came from small probes over hand-written input. Fixtures explain; the corpus discriminates. One further result, a stray token at the head of a shipped mod file, is visible only as a disagreement and is not reported by either adapter alone.

## Corpus

| Corpus | Files | Size | Definitions |
| --- | --- | --- | --- |
| Vanilla `Pegasus v4.4.6` | 4,578 | 53.5 MiB | 58,398 |
| Gigastructural Engineering & More (1121692237) | 1,792 | 33.5 MiB | 42,142 |
| Ancient Cache of Technologies (1419304439) | 1,120 | 17.7 MiB | 15,773 |
| Acquisition of Technology (2178603631) | 437 | 4.4 MiB | 5,090 |
| `fixtures/parser/valid` | 11 | 15 KiB | 51 |
| **Total** | **7,938** | **109.0 MiB** | **121,454** |

Selection is `.txt`, `.asset`, `.gfx`, `.gui`, and `.mod` under `common/`, `events/`, `interface/`, `gfx/`, `map/`, and `prescripted_countries/`, plus root-level descriptors. An allowlist, because the install also holds `licenses/`, `pdx_launcher/`, and an application bundle full of `.txt` files that were never script; counting a font licence as a parser failure would have inflated the failure rate with a number about nothing.

It is not a curation of *parseable* files. `common/HOW_TO_MAKE_NEW_SHIPS.txt`, `common/edicts/99_README_EDICTS.txt`, and `interface/credits.txt` are prose living in script directories, and they stay in, because what the parser does with them is a real question about failure isolation.

Localization `.yml` is excluded. It is a different language with its own module owner (`docs/technical-design.md:345`), and feeding it to a Clausewitz parser would manufacture failures that say nothing about either. Its encodings are covered through fixtures instead, because a byte order mark reaches script files too.

The resolver corpus is present as `fixtures/oracle/`, read and never modified — its checksums are pinned into every captured oracle record. The cases it establishes are restated in `fixtures/parser/` so the parser's tests do not depend on files frozen against another spike's evidence.

## Evidence matrix

| # | Requirement | Verdict | Record |
| --- | --- | --- | --- |
| 1 | Parse every syntactically valid file | **Met by the lexer, not by the tape.** The lexer reads all 7,938; the tape rejects 37, of which 29 are real script | `p1` |
| 2 | Preserve field order, duplicates, operators, mixed containers | **Met.** 65,234 duplicate top-level definitions retained in order; all eight operators; 1,261 mixed containers | `p1`, `p6` |
| 3 | Source ranges for definitions and facts, raw source retained separately | **Met by the lexer only.** 100% of scalars, containers, and definitions carry ranges; 0 of 8,690,226 fail to re-slice. The tape reaches 99.9998% of scalars and 0% of containers, through an unofficial technique | `p2` |
| 4 | Isolate a malformed file without losing the corpus | **Met, and the question was the wrong one.** The tape usually does not fail a malformed file — it reshapes it silently. Where a fault is detected, the lexer loses 1.0 definitions per file against the tape's 13.9 to 28.4, or the whole file | `p3` |
| 5 | Retain unknown keys and values without treating new primitives as syntax failures | **Met.** No schema, no key list; 96 real files carrying a byte order mark and two deliberately non-UTF-8 fixtures parse without a fault | `p1`, `p6` |
| 6 | A stable app-owned representation that does not expose Jomini beyond the boundary | **Met.** Enforced by a test that reads the model's own sources, with the two adapters as its negative control | `p1` |
| 7 | Parsing time and memory across vanilla and large mods | **Met.** Vanilla in 63 ms parallel, 211 ms serial; the wrapper costs 9–36% over the tape; 194 MiB peak for every corpus at once | `p5` |
| 8 | Scripted-constant definitions and references distinct enough for static base values | **Met.** Declaration, alias, and consuming reference all retained; the field after an unresolved reference stays visible | `p6` |
| 9 | Exact numeric lexeme; unresolved `@` distinguished from a block | **Met.** `0.1` and `0.10` stay distinct; `2200.1.1` is not a number; `cost = @unresolved` stays a reference where the game turns it into a block | `p6` |
| 10 | `inline_script` references, parameter bindings, and nesting preserved as syntax | **Met by the lexer, not by the tape.** Both call forms and fragment callees survive unexpanded; 720 conditional blocks retained, where the tape rejects the file or misreads the block | `p1`, `p6` |
| 11 | Both the enclosing block name and inner identifier fields preserved | **Met.** Component templates and sprite types both addressable by inner key under a shared block name | `p6` |
| 12 | The same representation regardless of root, enumeration order, or neighbour | **Met.** Digests identical across reversed order, parallel execution, and a byte-for-byte copy at a second absolute path | `p4` |

No requirement is unmet. Three are met by the lexer path and not by the tape, which is the decision.

## Findings

### The constraint this spike was written to test is true of one API and false of the library

The planned document stated that Jomini's public `TextToken` API exposes no byte ranges for successfully parsed structural tokens, that container `end` fields are token indexes, and that only borrowed scalars offer an unofficial offset derivation. Every clause of that is correct — and it is a statement about `TextTape`, not about Jomini.

`jomini::text::TokenReader` is publicly exported and documented, and `position()` returns the byte offset of the data stream consumed so far, for every token including braces and operators. `ReaderError::position()` gives a byte offset for a failure, and the reader is incremental, so it can be resumed rather than abandoned.

Both constraints the spike was written to evaluate — structural source ranges and whole-file failure — therefore have an answer inside supported public API. No fork, no patch, no vendored copy. This is recorded rather than edited into the constraints section because the reasoning that produced the original claim was sound and the correction is about scope: a conclusion drawn from one type in a library was stated as a conclusion about the library.

### Deriving a span from where a token ends is wrong, sporadically, and a small fixture would not have shown it

The obvious derivation is `start = position() - width`. It is wrong, and it is wrong in the way that is hardest to catch: usually right.

`examples/probe.rs` records the two behaviours that rule it out. Reading `position()` *before* a token gives an offset that has not yet consumed the whitespace and comments preceding it. Reading it *after* an unquoted token sometimes includes one trailing boundary byte and sometimes does not — for `"a = 1\nbb = 2\n"`, the first token over-runs by one and every later token does not.

The derivation that holds is to treat the position taken before the read as a lower bound and skip the trivia Clausewitz skips — a byte order mark, whitespace, `;`, and `#` comments to end of line — then take the width from the token itself.

`;` belongs in that list and was missing from the first attempt. Jomini's own character-class table marks it a token boundary, and vanilla writes it in `.gui` files. Leaving it out shifted every following span by its width in 7 files. The corpus round-trip caught it; the fixture suite did not, and could not have, because no fixture then contained a semicolon.

### Every byte range in 7,938 files re-slices to exactly the text it claims

| | Scalars | Containers | Top-level definitions |
| --- | --- | --- | --- |
| Nodes with a range | 7,127,428 | 1,441,344 | 121,454 |
| Coverage | 100% | 100% | 100% |
| Failed to re-slice | 0 | 0 | 0 |

Against the tape's unofficial borrowed-scalar derivation: 7,054,943 of 7,054,958 scalars located — 15 misses, so the technique is not even reliable for what it does reach — and **0 containers**, because no tape token carries the position of a brace. A Source Excerpt of a definition needs its braces.

The totals differ between the two because the tape only reaches the 7,901 files it accepts.

Definition spans are small enough to show: median 374 bytes in vanilla, 508 in Gigastructural Engineering, with a 95th percentile of 1.7 to 4.4 KiB. The maximum is 554,960 bytes, so bounded excerpts need a cap rather than a promise that definitions are small — a design note for `analysis`, not a parser problem.

### The tape does not fail a malformed file. It reshapes it, and says nothing

This is the finding that changed the decision, and it inverts the premise the spike was planned on.

The planned document expected `TextTape::from_slice` to fail a whole file on a syntax error, making file-level isolation the mitigation and per-file definition loss the number to measure. It does that only for faults that reach end of input. An unbalanced brace is accepted:

```text
first = { a = 1 }
broken = { a = 1
second = { b = 2 }
third = { c = 3 }
```

The tape returns two top-level definitions and no error. `second` and `third` are still in the tree — nested inside `broken`, under a parent the source never gave them. A stray extra `}` is likewise discarded in silence.

Two vanilla files ship in this condition. `gfx/models/effects/nomads.gfx` and `common/scripted_loc/scripted_loc_ruloc.txt` each have one more `{` than `}`, and one Gigastructural Engineering event writes `}= yes`. The tape accepts all three.

For this application a silent reshape is worse than a refusal. `docs/technical-design.md:332` requires an Analysis Issue to attach to the narrowest known evidence node, and a reshape offers no node and no signal — the documentation would simply be wrong somewhere, confidently. The lexer reports a fault with a byte offset in every one of these cases.

### One fault costs one definition, or it costs the file

Single-character faults injected at the first, middle, and last definition of 320 sampled files, against a clean parse of the same file. Mean definitions lost per malformed file:

| Fault | At | Cases | Tape kept | Tape lost | Tape rejected | Lexer kept | Lexer lost |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Remove a close brace | first | 103 | 3.3% | 28.4 | 2 | 96.5% | **1.0** |
| Remove a close brace | middle | 130 | 52.1% | 13.9 | 3 | 96.3% | **1.1** |
| Remove a close brace | last | 134 | 99.9% | 0.0 | 1 | 96.7% | **1.0** |
| Add a close brace | any | 417 | 100% | 0.0 | 0 | ~100% | **0.0** |
| Open an unterminated quote | first | 47 | 33.2% | 9.5 | 34 | 99.9% | **0.0** |
| Truncate the tail | last | 135 | 99.8% | 0.1 | 2 | 96.7% | **1.0** |

The positive control holds: truncating the last definition is a fault after which recovery can reach nothing, and the lexer loses exactly 1.0 definitions per file.

Two entries need reading carefully rather than at face value. The tape's 100% on *add a close brace* is the silent reshape above — it keeps the count and loses the structure, and reports nothing, while the lexer flagged all 417. And in the unterminated-quote rows the lexer occasionally ends with *more* top-level definitions than the intact file: resuming at a column-zero line inside a construct the fault had already opened promotes its contents to top level. Recovery is a layout heuristic, not a repair, and it can misattribute nesting around a fault. It never invents content — everything it yields still had to parse — but a definition recovered next to a fault is not evidence of the same quality as one parsed cleanly, and `analysis` should treat a file with faults accordingly.

Of 1,290 mutations attempted, 178 produced no fault in either adapter. They are reported and excluded from the averages rather than counted as faults that cost nothing.

### Vanilla ships syntax that Jomini's tape cannot parse

37 files are rejected outright. The classes:

| Count | Diagnostic | What it is |
| --- | --- | --- |
| 17 | `unexpected end of file` | Bare token lists such as `common/component_tags/00_tags.txt`; inline-script fragments whose entire content is `$code$`; and files whose braces balance but whose conditional blocks unbalance the tape's container stack, including `giga_scripted_effects.txt` at 3,500 matched braces |
| 12 | `expected start of parameter definition` | `@\[ … ]` escaped arithmetic over scripted constants |
| 7 | `unrecognized operator` | The `!` of a `[[!NAME]` conditional, and prose |
| 1 | `invalid syntax for token headers` | `common/gamesetup_settings/gamesetup_settings.txt` |

Twenty-nine of the 37 are real script content in vanilla and the measured mods. Six are prose in script directories, and two are this spike's own fixtures, written to reproduce the classes above. `common/scripted_effects/00_scripted_effects.txt`, `archaeology_event_effects.txt`, `first_contact_effects.txt`, and five more vanilla files are lost to the escaped-expression form alone. A parser that lost them would lose vanilla's scripted effects, which is where technology grants live.

Conditional compilation is the sharper case, because the tape half-supports it. `[[NAME] … ]` compiles a body in only when an inline-script argument is supplied, and the tape emits `Parameter` tokens for it — but only while the enclosing container is object-shaped. A bare flag such as `optimize_memory`, which opens hundreds of vanilla scripted effects, makes the container mixed, and from there the tape lexes `[[EXTRA]` as three separate scalars and turns `[[!EXTRA]`'s negation mark into an operator. The corpus holds 720 conditional blocks, 34 of them negated.

`TokenReader` does not recognize these constructs either. That is the wrapper's real cost, and it is small: 38 lines of code recognize `[[NAME]`, `[[!NAME]`, `]`, `@\[ … ]`, and `[Scope.GetName]` from the bytes and tell the reader to skip them, and 17 more skip trivia. The finding is not that Jomini is missing a feature — these are Stellaris dialect, not Paradox-wide — but that the wrapper cannot be a thin pass-through over Jomini's token stream, and any estimate that assumed it could was wrong by about 55 lines.

### Duplicate definitions are the normal case, not the exception

65,234 top-level definitions across the corpus repeat a key already used in the same file — more than half of all 121,454 definitions. `.gui` files repeat `containerWindowType`, `.asset` files repeat `entity`, `.gfx` files repeat `spriteType`, and `common/component_templates` repeats `utility_component_template` hundreds of times per file.

The resolver oracle established that duplicate resolution runs in *opposite* directions between registries — technologies, scripted triggers, and scripted effects keep the last registration; scripted constants and events keep the first (`docs/spikes/resolver-evaluation.md:113`). A parser that deduplicated would be wrong for one group whichever direction it chose, and wrong in the worst way: silently attributing the wrong body to the wrong source. At this volume, using Jomini's `ObjectReader` DOM — whose `field_groups` groups repeated keys — would not have been a convenience. It would have been the defect.

### Two of the eight operators exist in the evidence only because a fixture states them

Across 109 MiB of real source: `=` 3,954,423 times, `>=` 8,091, `>` 5,352, `<=` 5,069, `<` 4,473, `!=` 27. `==` and `?=` appear exactly once each, and both occurrences are in `fixtures/parser/valid/common/parser_operators.txt`.

The planned corpus listed comparison operators as a requirement, and the resolver corpus could not supply them — every file in `fixtures/oracle/` uses `=` and nothing else. Had the parser corpus been real source alone, two operators would have been reported as covered on the strength of a census that never saw them. This is the specific thing the "do not infer parser suitability from a small hand-written fixture" shortcut is aimed at, running in the other direction: a large real corpus is not automatically a complete one either.

### The cross-check found garbage in shipped source that neither adapter reports

`common/component_templates/acot_1_components_weapons_mutations_delta.txt` in Ancient Cache of Technologies begins with the literal bytes `3407`, before its first comment. The mod ships and loads that way.

Both adapters accept it. A stray scalar in a sequence of elided assignments shifts every subsequent pairing by one, differently in each adapter, and neither reports anything — the tape ends with 336 top-level definitions and the lexer with 326. Only the disagreement between them surfaced it.

That is the strongest argument for keeping the tape path in the harness. It is also a limitation to disclose plainly: for a class of fault that produces valid-looking source, this application has no signal of its own, and `fixtures/parser/malformed/stray_token_at_head.txt` records the case so it is not rediscovered.

### Determinism holds across order, parallelism, and absolute root

Per-file digests folded into one corpus digest are identical when the corpus is enumerated in sorted order and in reversed order, when parsing runs serially and through a work-stealing pool, and when Acquisition of Technology is copied byte for byte to a temporary directory at a different absolute path and re-parsed there — 437 of 437 files matching.

The copy is the control that matters. Order and parallelism can only catch state leaking between files; only a genuinely different root can catch a path reaching the model. It is a copy rather than a symlink deliberately, because a symlink shares an inode and would pass even if the code canonicalized the path.

### The wrapper costs 9 to 36% of parse time, and the parser is not the build's problem

Median of five repeats, first run of each corpus discarded, bytes read up front so the disk is not being timed:

| Corpus | Lexer serial | Lexer parallel | Tape serial | Tape parallel |
| --- | --- | --- | --- | --- |
| Vanilla, 53.5 MiB | 211 ms | 63 ms | 191 ms | 48 ms |
| Gigastructural, 33.5 MiB | 133 ms | 39 ms | 116 ms | 29 ms |

Roughly 250 MiB/s serial and 850 MiB/s parallel for the lexer. Across all four corpora the wrapper's overhead spans 8.6% (Ancient Cache of Technologies, parallel) to 36.1% (Gigastructural Engineering, parallel); it is largest where per-file work is smallest, because the wrapper's extra cost is per token rather than per file. Peak resident set was 194 MiB with every corpus and both adapters held in memory at once, which a real build would not do.

Vanilla and one large mod parse together in about 100 ms in parallel. The same technical design records second-pass SHA-256 costs of 1.78 s for vanilla and 0.52 s for Gigastructural Engineering (`docs/technical-design.md:428`): **hashing the corpus costs an order of magnitude more than parsing it.** The parser does not constrain the build model, and the deferred choice between an awaited command and a host-owned job (`docs/technical-design.md:152`) should not be decided on parsing cost.

These are directional, in the sense that section already applies to the fingerprint numbers. They come from a spike harness rather than the real adapter inside a build.

## Rejected shortcuts

Each of these was available, and each is named with the measurement that would have taken it.

**Converting Paradox script to JSON as the parsed content model.** Ruled out before measuring, and the corpus census says why it stays ruled out: 65,234 duplicate top-level definitions have no JSON object representation, 1,261 mixed containers have no JSON shape at all, and 1,171,770 numeric lexemes would round through IEEE 754 doubles into values the game itself does not compute — the resolver oracle watched it compare `0.1 + 0.2` against `0.3` as exactly equal.

**Inferring suitability from a small hand-written fixture.** The fixture suite passed 15 of 15 adapter tests while the wrapper was still shifting every span after a semicolon, still reading the `]` of `[From.GetName]` as a conditional close, and still failing to recognize conditional blocks at all. All three were found by the corpus, none by a fixture, and two by files nobody would have thought to write. Fixtures were still worth writing — `==` and `?=` are in the evidence only because one states them — but they discriminate nothing on their own.

**Treating a successful parse as proof.** The tape parses `gfx/models/effects/nomads.gfx` successfully and gets it wrong. It parses the ACOT component file successfully and gets it wrong. Reading `tapeOK 4560/4578` as a coverage result would have recorded a 99.6% success rate for a path that silently reshapes real vanilla content. Success counts were never the measurement; agreement between two readings was.

**Driving the divergence count to zero.** Seven files still disagree between the adapters, and each was traced individually: three are conditional blocks the tape loses inside a mixed container, two are scope tokens the tape splits, one is documentation prose in a script directory, and one is the ACOT stray token. Making them agree would have meant reproducing the tape's misreadings in the wrapper. The count is not a score, and a lower one here would have been worse work.

**Measuring blast radius as "does the rest of the corpus still index".** It always does, in both paths, which is why the planned document asked for definitions lost per file instead. That instruction is the reason the silent-reshape finding exists: a file-level pass/fail would have recorded the tape as isolating faults perfectly.

## Completion model

### Evidence collection

**Complete.** All twelve requirements are answered against the pinned build with captured, drift-gated records.

### Adapter conformance

**Partial, and deliberately so.** The harness adapter is throwaway. It demonstrates the approach and produces the measurements; it is not the `analysis::parser` implementation, and the seven residual divergences are documented behaviour rather than open defects. The real adapter inherits the design, the fixture corpus, and the cross-check technique.

### Known limitations, carried forward

- Recovery is a layout heuristic. It resumes at the next column-zero identifier line, which is a convention of Stellaris source rather than a rule of the grammar, and around a fault it can promote nested content to top level. Definitions recovered from a faulted file warrant an Analysis Issue on the file, not just on the fault site.
- A stray token that produces valid-looking source is undetectable by either adapter alone.
- Bounded excerpts need an explicit size cap. The largest single definition is 554,960 bytes.
- `.yml` localization is out of scope here and belongs to the localization module's own evaluation.

### Current standing

| Dimension | Standing |
| --- | --- |
| Syntax coverage | **Complete.** All 7,938 files, all 8 operators, every container shape, three encodings |
| Source traceability | **Complete.** 100% span coverage, 0 round-trip failures over 8,690,226 ranges |
| Failure isolation | **Complete.** 1.0 definitions lost per detected fault, with the detection gap disclosed |
| Determinism | **Complete.** Order, parallelism, and absolute root |
| Performance | **Established, directional.** Parsing is an order of magnitude cheaper than the hashing beside it |
| Semantic preservation | **Complete.** 7 of 7 fixture checks, requirements 2, 5, 8–11 |
| Real adapter | **Not started.** This harness is not it |

Comments are not semantically preserved, which the planned document permits: they remain available through bounded excerpts from the retained raw source, and the spans measured here are what makes that possible.

## Captured records

| Run | Answers | Artifacts |
| --- | --- | --- |
| `p1-coverage` | 1, 2, 5, 6 | `coverage.json`, `divergences.txt`, `tape-failures.txt`, `span-faults.txt`, `lexer-recoveries.txt` |
| `p2-ranges` | 3 | `ranges.json` |
| `p3-blast` | 4 | `injected.json`, `fixtures.txt`, `sampling.txt` |
| `p4-determinism` | 12 | `determinism.json` |
| `p5-perf` | 7 | `timings.json`, `memory.txt` |
| `p6-semantics` | 2, 5, 8, 9, 10, 11 | `checks.txt` |

Each record holds a manifest pinning the installed Stellaris build, the Jomini and rustc versions, the operating system and architecture, a content digest per corpus with its file count and byte total, a SHA-256 for every emitted artifact, and the run's purpose stated in full. Per-file digests are recorded only for corpora committed to this repository; the game corpora are pinned by tree digest, because listing 7,927 proprietary paths in every record would add a megabyte of duplicated JSON for a detail that re-running `coverage` supplies.

The harness lives in `tools/parser-spike/` and is not a workspace member of `src-tauri`. [ADR 0007](../adr/0007-parse-stellaris-source-through-a-wrapped-incremental-lexer.md) accepts Jomini; the dependency enters the application's Cargo graph when the production parser adapter is implemented. The fixture corpus, cross-check technique, and captured expectations are referenced by the implementation plan.
