# MVP acceptance

The MVP is accepted when one end-to-end technology-documentation slice handles all five golden cases below on the Desktop Host and exposes each generated result through a Companion-Ready Cache. These cases are the primary end-to-end acceptance harness, not illustrative examples.

## Ordinary drawable technology

A representative vanilla technology with a nonzero Draw Weight must prove the ordinary path:

- Multi-language localization with defined fallbacks.
- Technology icon rendering.
- Resolved base cost.
- Prerequisite technologies.
- Eligibility requirements and blockers.
- Base Draw Weight and conditional Weight Modifiers.
- Content unlocked by the technology.
- Bounded Source Excerpts.

The exact fixture will be pinned when implementation begins.

## Conditional zero-weight vanilla technology

`tech_combat_computers_3` (Sapient Combat Simulations) must prove that eligibility and drawability remain distinct:

- The technology remains documented as otherwise eligible when its requirements are met.
- The AI Outlawed policy condition is shown as a `×0` Weight Modifier.
- The page prominently explains that the technology will not appear through normal random research while that condition applies.
- Other positive and negative Weight Modifiers remain visible.

## ACOT Enigmalith

`tech_precursor_enigmalith` and its related megastructure must prove the modded multi-route path:

- A search for Enigmalith returns separately typed technology and megastructure entries.
- The technology's zero base Draw Weight is prominent.
- The Precursor Databank Grant Site appears as a route card.
- Its Unlock Effects state that it adds the research option and 20% research progress.
- The Final Spark Grant Site appears as a separate route card with its origin-specific conditions.
- Its Unlock Effect states that it adds the research option.
- Route Summaries use Player-Facing Anchors and keep internal flags and bookkeeping in the linked technical trace.
- Scripted constants resolve to meaningful base values where applicable.

## Malformed source

A deliberately malformed fixture must prove partial generation:

- The malformed input produces an Analysis Issue with its source location and reason where available.
- Successfully analyzed content remains searchable and documented.
- The Target Mod carries a completeness warning.
- Affected pages or sections carry narrower warnings when the impact can be traced.
- No failed input is silently omitted or guessed.

## Technology redefinition

A controlled fixture that defines the same technology identifier in Vanilla Content and the Target Mod must prove entry identity and effective resolution:

- Search and browse expose one Entry Key rather than duplicate documents.
- The effective documentation matches a recorded observation from the pinned Stellaris build, checksum, operating system, DLC set, mod descriptor and load order used by the resolver oracle.
- Source provenance identifies both definitions and which facts contributed to the effective entry.
- The fixture includes a redefinition that omits `potential`, so the resolver cannot pass by assuming unconditional whole-object replacement.
- The oracle record names every compared effective field and the raw evidence used to establish the expected outcome.

## Companion verification

Each generated case must be readable from an authorized Companion Device without triggering parsing, generation, asset conversion, cache mutation, or exposure of absolute host paths.
