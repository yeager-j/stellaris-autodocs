# Parser evaluation

## Status

Planned. Jomini is the front-runner, not yet an accepted dependency.

## Hypothesis

Jomini can provide the syntax foundation for deterministic Stellaris Mod Source analysis when wrapped behind an app-owned parser interface and adapted into a provenance-preserving parsed content model.

## Corpus

Evaluate the parser against:

- Every vanilla file needed by the technology vertical slice.
- The equivalent files from several large installed mods.
- Scripted triggers, scripted effects, events, and other files referenced by those technologies.
- Representative files containing duplicate keys, comparison operators, scripted variables, headers, arrays, objects, and mixed containers.
- Deliberately malformed fixtures used to verify failure isolation and diagnostics.
- The source needed by the agreed `tech_combat_computers_3` and ACOT Enigmalith MVP acceptance cases.
- The complete technology-redefinition game-oracle fixture from the [resolver evaluation](./resolver-evaluation.md).

## Required evidence

The spike must establish whether the adapter can:

1. Parse every syntactically valid file in the evaluation corpus.
2. Preserve field order, duplicate fields, operators, and mixed containers.
3. Record source ranges sufficient to show bounded excerpts for top-level definitions and recognized facts while retaining raw source separately.
4. Isolate a malformed file without preventing other files from being indexed.
5. Retain unknown keys and values without treating new game primitives as syntax failures.
6. Produce a stable app-owned representation that does not expose Jomini types beyond the parser boundary.
7. Measure parsing time and memory use across vanilla and representative large mods.
8. Preserve scripted-constant definitions and references distinctly enough for the resolver to produce static base values.

Semantic preservation of comments is not required. Comments need only remain available through bounded excerpts from the retained raw source.

## Known Jomini constraints to evaluate

Jomini's current public `TextToken` API does not expose byte ranges for successfully parsed structural tokens. Container `end` fields are token indexes, not source positions. Borrowed scalar tokens may permit offsets to be derived from their position within the original input buffer, but that is an unofficial technique and does not by itself locate braces, operators, or complete definitions.

`TextTape::from_slice` also returns one error for a malformed file rather than resuming at a later top-level definition. File-level isolation still protects the rest of the corpus, but one syntax error may make every otherwise valid definition in a large file unavailable.

The spike must prototype and assess the maintainability of a source-position-aware wrapper or bounded Jomini extension. It must also record how many definitions are lost per malformed corpus file, not merely whether other files continue to parse.

## Rejected shortcuts

- Do not convert Paradox script to JSON as the parsed content model.
- Do not infer parser suitability from a small hand-written fixture.
- Do not treat successful parsing alone as proof that source traceability and deterministic semantics are supported.

## Outcome

Conclude with one of:

- Adopt Jomini without modification and record the decision in an ADR only if source traceability is satisfied through supported APIs.
- Extend or wrap Jomini to close specific, bounded gaps.
- Evaluate a source-position-aware alternative if traceability or failure isolation cannot be achieved cleanly.
