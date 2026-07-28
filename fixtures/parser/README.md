# Parser fixtures

Hand-written source for the [parser evaluation](../../docs/spikes/parser-evaluation.md). They
cover what the real corpus cannot: syntax the game ships too rarely to rely on, and faults
that no shipped file may contain on purpose.

## Design

Two directories, because they answer opposite questions.

| Directory | Claim | Used by |
| --- | --- | --- |
| `valid/` | Every file parses cleanly through **both** adapters and yields the same model | `p1-coverage`, `p6-semantics`, production parser tests |
| `malformed/` | Every file contains **exactly one** deliberate fault | `p3-blast`, production recovery tests |

One fault per file is the whole discipline of `malformed/`. The resolver spike learned this
the expensive way: when two risky scripted constants shared a file, the first failure
cascaded and the second result became unattributable
(`fixtures/oracle/README.md`, "Isolation"). A blast-radius number measured against a file
with two faults would not say which one cost what.

Every `malformed/` file states, in its header comment, what the fault is, where it is, and
how many definitions each adapter should still see. That prediction is written before the
measurement, so a surprising number is a finding rather than a retrofitted expectation.

## Why these cases

The resolver corpus already exercises duplicate registrations, whole-file collisions,
scripted constants, and inline scripts, and the spike names it as an input. These fixtures
fill in what it does not reach:

- **Comparison operators.** Every file in `fixtures/oracle/` uses `=` and nothing else. The
  real corpus does use `<`, `<=`, `>`, `>=`, and `!=`, but not `==` or `?=`, so those two
  would go unexercised without a fixture that states them deliberately.
- **Encodings.** A byte order mark, a Windows-1252 byte, and an invalid UTF-8 byte. The
  spike's requirement 5 is about not mistaking unfamiliar input for a syntax failure, and a
  non-UTF-8 byte in a mod file is exactly that.
- **Conditional compilation.** `[[NAME] … ]`, `[[!NAME] … ]`, and the empty `[[NAME]]`.
  These decide whether a definition's fields are present at all, and the two Jomini APIs
  disagree about them.
- **Semantic cases restated.** Inline scripts, scripted constants, and inner-field keys
  appear here as well as in `fixtures/oracle/`, so the parser tests do not depend on files
  frozen against a different spike's evidence. The oracle fixtures are read, never modified:
  their checksums are pinned into every captured oracle record and `tools/oracle/verify.py`
  enforces it.

## Licensing

Every file here is original work for this repository. No Stellaris content is copied.
Fixtures use vanilla-shaped identifiers and structures to be representative, and comments
quote short field snippets where needed to explain what is being tested, but no vanilla file
is reproduced.

## Running

These files are read by the parser's own tests, from `src-tauri/`:

```bash
cargo test --features test-support analysis::parser
```

The `valid/` tree is also established as a corpus in its own right — it is laid out as a mod
so it enumerates through exactly the production path a real one does — and the conformance
harness runs its cross-check and its digest controls over it on every ordinary test run
(`src-tauri/src/analysis/parser/conformance/`). That is what keeps those gates exercised on a
machine with no Stellaris installed.

The blast-radius measurement that shaped the `malformed/` expectations was captured by
`tools/parser-spike`, which no longer exists; `docs/spikes/parser-evaluation.md` records what
it found and why it was deleted. The property-based equivalent now lives beside the parser as
`one_injected_fault_loses_at_most_its_definition`.
