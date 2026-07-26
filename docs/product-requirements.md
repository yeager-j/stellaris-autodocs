# Stellaris Mod Documentation — Product Requirements

Status: Draft

Last updated: 2026-07-25

## Problem Statement

Stellaris players routinely need answers to questions such as:

1. How do I unlock this technology, building, ship component, or megastructure?
2. How do I begin the Event Chain that leads to it?

Mod documentation is often incomplete, outdated, or absent. Even when documentation exists, it may not capture conditional Draw Weight, zero-weight states, indirect event grants, alternate origin-specific routes, or changes introduced by newer mod versions.

The installed Mod Source is the closest thing to an authoritative record because Stellaris content is distributed as readable Paradox script. However, reading it manually requires specialized knowledge and substantial effort. A player may need to search across technologies, events, special projects, scripted triggers, effects, flags, localization, and assets. Simple text search produces misleading results because it cannot distinguish checks from grants, cannot explain causal chains, and may include unrelated installed or disabled mods.

A conventional web application is a poor fit. It would require users to upload local mods or limit the service to a curated catalog, and browser filesystem access is not sufficiently universal or robust. The product therefore needs to work directly with local Stellaris installations while remaining easy to use during play.

The primary audience is players. Mod-author inspection and Documentation Export are valuable secondary use cases, but they do not organize the MVP.

## Solution

Build a local-first desktop application that generates deterministic Player Documentation from a selected Target Mod and the matching Vanilla Content installed on the same computer.

On first launch, the application detects proposed Discovery Locations, presents a confirmable setup screen, and builds a unified Mod Library. The player selects one Target Mod, after which the application parses and resolves that mod against the base game and locally available DLC.

The main experience is search-first. Search spans technologies, megastructures, buildings, and ship components while allowing category filtering. Technology receives deep Player Documentation in the MVP; the other categories may initially appear as thinner Searchable Entries that expose their direct technology gates and lead into the documented technology path.

A technology page explains, in structured and traceable form:

- Prerequisite technologies.
- Eligibility requirements and blockers.
- Base Draw Weight.
- Every conditional Weight Modifier.
- Conditions that reduce Draw Weight to zero.
- Direct Grant Sites and their precise Unlock Effects.
- All discovered Unlock Paths.
- Content made available after research.
- Resolved Base Values where source constants can be resolved safely.
- Bounded Source Excerpts for technical inspection.

The product retains the known causal graph but presents it through concise Route Summaries bounded by Player-Facing Anchors. Internal flags, variables, and bookkeeping events remain available in a technical trace without overwhelming the primary explanation. Each Grant Site produces one route card in the MVP, and effects from the same enclosing action are combined.

The application also provides an optional Companion Mode. After temporary QR-code authorization, a phone or other Companion Device on the same local network can use the same responsive interface to browse Companion-Ready Caches. Companion Devices remain read-only and cannot trigger parsing, generation, asset conversion, configuration changes, or filesystem writes.

### MVP success

The MVP succeeds when its end-to-end behavior passes the five agreed golden cases:

1. A representative ordinary drawable vanilla technology.
2. `tech_combat_computers_3` (Sapient Combat Simulations), including its conditional `×0` Draw Weight behavior when AI is outlawed.
3. ACOT's Enigmalith, including multi-category search and distinct Precursor Databank and Final Spark Grant Sites.
4. A deliberately malformed fixture that still produces useful Incomplete Documentation with explicit warnings.
5. A controlled Vanilla-plus-Target-Mod technology redefinition, including an omitted `potential` field, whose effective definition and provenance match a pinned Stellaris game oracle.

All generated cases must also be readable through a Companion-Ready Cache.

## User Stories

1. As a Stellaris player, I want the application to detect my Stellaris Installation, so that I do not need to find it manually.
2. As a Stellaris player, I want detected Discovery Locations to be pre-filled, so that first-run setup requires minimal effort.
3. As a Stellaris player, I want to review and edit detected paths before confirming them, so that I can correct unusual installation layouts.
4. As a Stellaris player, I want one confirmation action to finish a valid default setup, so that I can reach the Mod Library quickly.
5. As a Stellaris player, I want the Mod Library to combine mods from multiple Discovery Locations, so that Workshop and local installations appear together.
6. As a Stellaris player, I want every discovered Mod Installation to appear regardless of launcher state, so that I can document any installed mod.
7. As a Stellaris player, I want physically separate copies of the same mod to remain separate, so that I can choose between a Workshop copy and a modified local copy.
8. As a Stellaris player, I want each Mod Installation to show its Discovery Location and path, so that I can distinguish otherwise identical entries.
9. As a Stellaris player, I want declared game-version incompatibility to appear as a warning rather than a blocker, so that stale metadata does not prevent analysis.
10. As a Stellaris player, I want Declared Dependencies to be visible, so that I understand why cross-mod references may be unresolved.
11. As a Stellaris player, I want dependency metadata to remain advisory, so that selecting one Target Mod does not silently compose a partial Playset.
12. As a Stellaris player, I want to select one Target Mod for analysis, so that the resulting documentation has an understandable scope.
13. As a Stellaris player, I want the Target Mod analyzed against my installed base game and DLC, so that shared Vanilla Content can be resolved.
14. As a Stellaris player, I want provenance retained for effective definitions, so that I can see whether behavior comes from Vanilla Content or the Target Mod.
15. As a Stellaris player, I want cross-mod references outside the selected scope shown as Unresolved References, so that missing context is not silently invented.
16. As a Stellaris player, I want documentation generated only when I first open a Target Mod, so that initial discovery remains fast.
17. As a Stellaris player, I want unchanged documentation to load from cache, so that revisiting a mod is fast.
18. As a Stellaris player, I want edits, additions, deletions, and renames in Mod Source to invalidate stale documentation, so that the cache reflects actual content.
19. As a Stellaris player, I want game, DLC, parser, resolver, generator, search-index, localization, and asset-conversion changes to invalidate incompatible caches, so that cached documentation remains trustworthy.
20. As a Stellaris player, I want a manual Refresh action, so that I can explicitly recheck source after an update.
21. As a Stellaris player, I do not want constant live filesystem watching during the MVP, so that the player-focused application remains simple.
22. As a Stellaris player, I want one search box across supported content categories, so that I do not need to know what kind of object I am searching for.
23. As a Stellaris player, I want search results typed as technology, megastructure, building, or ship component, so that identically named concepts remain distinguishable.
24. As a Stellaris player, I want to filter search by content category, so that I can narrow broad results.
25. As a Stellaris player, I want Target Mod content shown by default, so that Vanilla Content does not overwhelm mod-focused results.
26. As a Stellaris player, I want an option to include Vanilla Content, so that I can investigate shared or modified definitions when needed.
27. As a Stellaris player, I want search to match names in my selected language, so that I can use the terminology shown in my game.
28. As a Stellaris player, I want English names to remain searchable, so that guides and community discussions remain useful in another configured language.
29. As a technically inclined player, I want raw script identifiers and localization keys to be searchable, so that source references lead to the right documentation.
30. As a Stellaris player, I want partial-name and minor-spelling-error matching, so that I can find content without remembering the exact title.
31. As a Stellaris player, I want exact and prefix name matches ranked above fuzzy or identifier matches, so that likely results appear first.
32. As a Stellaris player, I want descriptions and raw source excluded from primary search, so that the result list is not flooded with incidental mentions.
33. As a Stellaris player, I want a megastructure search result to identify its required technology, so that I can follow the actual unlock path.
34. As a Stellaris player, I want a technology page to show its localized name, description, and icon, so that I can recognize it in the game.
35. As a Stellaris player, I want technology constants resolved into meaningful base values, so that internal variable names do not replace useful information.
36. As a Stellaris player, I want runtime-dependent or ambiguous values labeled rather than guessed, so that base values are not mistaken for my live game values.
37. As a Stellaris player, I want prerequisite technologies listed separately, so that I understand the technology chain.
38. As a Stellaris player, I want eligibility requirements represented as structured “All of,” “Any of,” and negated groups, so that source logic remains understandable.
39. As a Stellaris player, I want blockers called out clearly, so that I can see why a technology cannot currently become eligible.
40. As a Stellaris player, I want Base Draw Weight separate from eligibility, so that I do not confuse entry into the pool with likelihood of selection.
41. As a Stellaris player, I want every supported Weight Modifier listed with its condition, so that I understand what raises or lowers relative drawability.
42. As a Stellaris player, I want `×0` Weight Modifiers given special prominence, so that I know when normal random research cannot select a technology.
43. As a Stellaris player, I want zero Draw Weight explained even when the technology appears in `techweights`, so that console output does not mislead me.
44. As a Stellaris player, I want the application to avoid claiming an absolute research probability without live state, so that relative weights are not presented as exact chances.
45. As a Stellaris player, I want technology pages to show what research makes available, so that I understand the reward for completing a technology.
46. As a Stellaris player, I want direct effects distinguished as adding a research option, adding research progress, completing a technology, or changing Draw Weight, so that “unlocks” is not ambiguous.
47. As a Stellaris player, I want multiple Unlock Effects from one action shown together, so that one Grant Site does not become several misleading routes.
48. As a Stellaris player, I want each discovered Grant Site represented, so that alternate acquisition methods are not discarded.
49. As a Stellaris player, I want the Precursor Databank and Final Spark Enigmalith routes shown separately, so that origin-specific alternatives remain visible.
50. As a Stellaris player, I want concise Route Summaries based on recognizable in-game actions, so that I do not need to understand internal event plumbing.
51. As a Stellaris player, I want required selections and direct requirements shown with a Route Summary, so that the guidance is actionable.
52. As a Stellaris player, I want prerequisite Unlock Paths linked rather than recursively expanded in one page, so that complex Event Chains remain navigable.
53. As a technically inclined player, I want internal flags, variables, scripted indirection, and bookkeeping events retained in a technical trace, so that I can verify the summary.
54. As a Stellaris player, I want route guidance to avoid unnecessary probability simulation, so that the MVP answers what to do without overcomplicating the page.
55. As a Stellaris player, I want every discovered route shown initially, so that uncertain player, AI, debug, or demo classifications do not hide useful information.
56. As a Stellaris player, I want to hide an irrelevant route, so that debug or duplicate-looking paths do not clutter my page.
57. As a Stellaris player, I want to see how many routes are hidden and restore them, so that hiding is reversible.
58. As a Stellaris player, I want hidden-route preferences to persist for an unchanged Target Mod, so that repeated cleanup is unnecessary.
59. As a Stellaris player, I want a materially changed route to become visible again, so that a stale hiding preference does not conceal new behavior.
60. As a technically inclined player, I want bounded Source Excerpts around documented facts, so that I can inspect evidence without opening a 6,000-line file.
61. As a Stellaris player, I want source comments excluded from authoritative generated facts, so that documentation reflects executable behavior.
62. As a technically inclined player, I want comments within a Source Excerpt left visible, so that nearby author context remains available for manual inspection.
63. As a Stellaris player, I want unsupported primitives shown explicitly, so that the application never disguises an interpretation gap.
64. As a Stellaris player, I want useful content to remain available when one file is malformed, so that a local failure does not invalidate an entire large mod.
65. As a Stellaris player, I want a Target Mod-level completeness warning when analysis is partial, so that I do not mistake Incomplete Documentation for a complete result.
66. As a Stellaris player, I want narrower warnings on affected pages when impact is known, so that unaffected documentation remains trustworthy.
67. As a technically inclined player, I want each Analysis Issue to identify its source and reason where possible, so that I can investigate it.
68. As a Stellaris player, I want all available localizations preserved, so that changing language does not require reparsing authoritative content.
69. As a Stellaris player, I want localization to fall back from my selected language to English and then to the raw key, so that missing translations degrade visibly.
70. As a Stellaris player, I want known Stellaris color and style markers rendered cleanly, so that localization remains readable outside the game.
71. As a Stellaris player, I want Static Localization References resolved with cycle detection, so that nested labels display meaningful text without hanging.
72. As a Stellaris player, I want known inline icons rendered with readable fallbacks, so that resource and concept text remains understandable when an asset is absent.
73. As a technically inclined player, I want Runtime Localization Tokens and unknown markup left visible, so that unavailable live values are not guessed or erased.
74. As a Stellaris player, I want missing or unsupported DDS assets replaced with clear placeholders, so that one icon cannot break a page.
75. As a Stellaris player, I want an optional Companion Mode, so that I can consult documentation on my phone without leaving the game.
76. As a Stellaris player, I want Companion Mode disabled until I enable it, so that the application does not expose a network service unexpectedly.
77. As a Stellaris player, I want to authorize my phone by scanning a QR code, so that connecting is quick and does not require an account.
78. As a Stellaris player, I want companion authorization to expire when Companion Mode ends or the desktop application exits, so that old sessions do not remain valid.
79. As a companion user, I want the responsive interface to provide the same search and documentation experience, so that I do not need a separate mobile app.
80. As a companion user, I want the Mod Library to show Ready, Needs build, and Out of date states, so that I know what can be opened remotely.
81. As a companion user, I want to switch among Companion-Ready Caches, so that I can browse previously generated mods without changing the desktop's active Target Mod.
82. As a companion user, I want a missing or stale cache to tell me to build it on the Desktop Host, so that remote browsing never initiates unexpected processing.
83. As a Desktop Host user, I want Companion Devices unable to change paths, settings, caches, or filesystem state, so that companion access remains read-only.
84. As a companion user, I want bounded Source Excerpts available, so that technical evidence remains accessible on my phone.
85. As a Desktop Host user, I want companion source paths made relative to the Mod Installation, so that absolute host paths remain private.
86. As a Desktop Host user, I want arbitrary file access and desktop-only file actions excluded from the companion API, so that authorization is narrowly scoped.
87. As a macOS user, I want the functional MVP to run on my platform, so that the product can be validated quickly against real installed mods.
88. As a Windows or Linux user, I want the public release tested on my platform, so that the architecture's cross-platform promise is real.
89. As a prospective contributor, I want the project released under MIT, so that contribution, modification, and redistribution terms are straightforward.

## Implementation Decisions

This section records product-shaping constraints already accepted during discovery. Module boundaries, schemas, APIs, storage formats, framework composition, and implementation sequencing are intentionally deferred to a separate technical-design pass.

1. The product is a Tauri desktop application with a responsive React interface.
2. The desktop window and Companion Devices use the same React application. The desktop reads through Tauri commands; Companion Devices receive the application and read-only documentation through an embedded HTTP service while Companion Mode is enabled.
3. The authoritative pipeline is parser and indexer, parsed content, content resolver, provenance-preserving resolved content model, and deterministic documentation generator.
4. The documentation generator receives resolved content and does not implement mod load-order semantics.
5. The MVP resolves one Target Mod against base-game and locally available DLC Vanilla Content.
6. Full Playset composition is a future resolver capability, not a second documentation pipeline.
7. Effective definitions retain provenance and future override history rather than being flattened without origin information.
8. Generated facts are deterministic and rule-based. AI is neither required nor authoritative.
9. Jomini is the provisional parser front-runner, subject to the separate real-corpus parser spike.
10. The parsed representation remains application-owned and must not expose a parser dependency throughout the product.
11. The parser preserves field order, duplicate fields, operators, mixed containers, unknown constructs, scripted-constant references, and source ranges required by the product.
12. Raw source is retained separately from the semantic model to support bounded Source Excerpts.
13. Source comments do not contribute generated facts.
14. Analysis failures are isolated so supported content can still produce Incomplete Documentation.
15. The known causal graph retains internal state and indirection, while the primary presentation is projected through Player-Facing Anchors.
16. The MVP creates one route card per Grant Site and combines co-located Unlock Effects from the same enclosing action.
17. The MVP does not semantically merge equivalent Grant Sites or classify routes by player, AI, debug, demo, or console audience.
18. Hidden Routes remain part of generated knowledge and the technical view.
19. Search is unified across technologies, megastructures, buildings, and ship components.
20. Technologies receive deep documentation; the other initial categories may use thinner Searchable Entries with direct technology gates.
21. Search indexes selected-language names, English names, script identifiers, localization keys, partial names, and typo-tolerant matches.
22. Search does not index long descriptions or raw source in the MVP.
23. Vanilla Content is excluded from results by default but can be included through filters.
24. All available localizations are preserved, with selected language, English, and raw-key fallback order.
25. MVP localization rendering supports style markers, Static Localization References, and known inline icons.
26. Runtime Localization Tokens, concept links, formatted runtime values, and unknown markup remain visibly raw.
27. DDS assets are decoded by the Desktop Host into browser-safe PNG or WebP representations and cached.
28. Missing or unsupported assets degrade to explicit placeholders.
29. Setup detects proposed Discovery Locations but allows user correction.
30. The Mod Library unifies multiple Discovery Locations while preserving each physical Mod Installation as a separate entry.
31. Launcher enabled state, Declared Dependencies, declared versions, and declared game compatibility are advisory metadata rather than analysis identity.
32. Documentation is generated lazily for a Target Mod.
33. Cache identity uses normalized relative paths and content, Vanilla/DLC content, and analysis-component versions rather than declared mod version.
34. The MVP checks fingerprints at defined user or application events and does not continuously watch source directories.
35. Each completed build publishes one immutable Documentation Revision atomically; failed or cancelled builds leave the previous revision readable on the desktop.
36. Companion Mode is explicit and normally disabled.
37. Companion authorization uses a temporary random secret delivered through a QR-code flow.
38. Companion credentials expire when Companion Mode is disabled or the desktop application exits.
39. Companion Mode is a proportionate access gate over local HTTP, not an account system or a claim of protection against a hostile local network.
40. Companion Devices can read only Companion-Ready Caches and cannot initiate analysis, generation, asset conversion, or cache mutation.
41. Companion Devices can switch among Ready mods without changing the desktop's active Target Mod.
42. Companion source access is limited to cached Source Excerpts and mod-relative paths.
43. Absolute host paths, arbitrary filesystem endpoints, configuration mutation, and desktop-only file actions are excluded from the companion boundary.
44. The functional MVP targets macOS, while the first public release remains a Windows, macOS, and Linux target.
45. The project is open source under MIT; project identity and copyright wording remain undecided.

## Testing Decisions

Tests should assert user-visible behavior and durable product contracts rather than internal implementation structure. The preferred primary seam is the highest practical one: a controlled Stellaris/Mod Source corpus enters the local application and produces searchable Player Documentation that can also be read through a Companion-Ready Cache.

The later technical design may introduce focused lower seams where failures cannot be diagnosed economically through the full product, but it should avoid duplicating the same behavior across many layers.

### Required acceptance coverage

1. The ordinary drawable-technology case verifies localization, icon handling, Resolved Base Value, prerequisites, eligibility, blockers, Draw Weight, Weight Modifiers, unlocked content, and Source Excerpts.
2. The `tech_combat_computers_3` case verifies that its AI-outlawed conditional `×0` Weight Modifier remains distinct from eligibility and produces the prominent non-drawable explanation.
3. The ACOT Enigmalith case verifies:
   - Separate technology and megastructure search results.
   - Zero base Draw Weight.
   - Separate Precursor Databank and Final Spark Grant Sites.
   - A Databank Unlock Effect that adds the research option and 20% progress.
   - A Final Spark Unlock Effect that adds the research option.
   - Origin-specific conditions.
   - Concise Route Summaries with internal mechanics retained in the technical trace.
4. The malformed-source case verifies that an Analysis Issue does not prevent unaffected content from remaining searchable and documented.
5. The technology-redefinition case verifies the oracle-backed effective fields and contributed, inherited, defaulted, duplicate, and shadowed provenance.
6. Every golden case verifies the equivalent read-only companion result from a Companion-Ready Cache.

### Behavioral test areas

1. First-launch detection, path correction, and Mod Library creation.
2. Multiple Discovery Locations and duplicate Mod Installation identity.
3. Advisory compatibility and dependency warnings.
4. Content-fingerprint invalidation for edits, additions, deletions, renames, referenced asset-byte changes, Vanilla/DLC updates, and analysis-version changes.
5. Search matching, ranking, category filtering, language behavior, provenance, and default Vanilla filtering.
6. Structured logical rendering of requirements and blockers.
7. Eligibility, Draw Weight, Weight Modifier, and Drawable distinctions.
8. Grant Site discovery and exact Unlock Effect normalization.
9. Multiple route preservation, manual hiding, restoration, and invalidation of stale hidden-route preferences.
10. Static constant resolution and explicit ambiguous/runtime fallbacks.
11. Localization fallback, recursive-reference cycle handling, style rendering, icon fallback, and raw runtime-token preservation.
12. DDS conversion across observed RGB/RGBA and DXT1, DXT3, and DXT5 inputs.
13. Partial generation, completeness warnings, diagnostic source ranges, and unsupported-primitive presentation.
14. Companion enablement, sequential-device QR pairing, secret rotation and expiry, connection troubleshooting, and rejection of unauthorized requests.
15. Derived Unavailable, Needs build, Checking, Out of date, Corrupt, Ready, and Incomplete states across startup verification, opening, refresh, build, and active companion browsing.
16. Enforcement that companion access cannot trigger writes or expose absolute host paths.
17. Responsive usability on a phone-sized browser and the desktop window.
18. Atomic Documentation Revision publication across successful, incomplete, failed, and cancelled builds with concurrent readers.
19. Real-machine packaging and smoke testing on macOS, Windows, and Linux before public release, including macOS Documents-folder access grant and denial.

### Parser spike evidence

Before parser adoption, the real-corpus spike must demonstrate:

1. Successful parsing of all syntactically valid files in the evaluation corpus.
2. Preservation of field order, duplicate fields, operators, and mixed containers.
3. Source ranges sufficient for definitions and recognized facts.
4. Failure isolation for malformed files, including the number of otherwise valid definitions lost when one large file fails as a unit.
5. Retention of unknown keys and values.
6. A stable application-owned representation.
7. Acceptable parsing time and memory use across Vanilla Content and representative large mods.
8. Preservation of scripted-constant definitions and references.

## Out of Scope

The following are outside this MVP PRD:

1. Technical design, including final module boundaries, schemas, API contracts, storage formats, and framework composition.
2. An implementation plan, issue breakdown, or delivery sequence.
3. Full Playset composition, override conflict analysis, and merged multi-mod documentation.
4. Save parsing or personalized guidance based on the player's current empire state.
5. Deep Player Documentation for content categories beyond technology.
6. Search categories beyond technologies, megastructures, buildings, and ship components.
7. Full Event Chain documentation as an independent destination.
8. Simulation of Event Chain progress, cumulative probabilities, expected values, minimum attempts, or outcome bounds.
9. Automatic classification of player, AI-only, debug, demo, console-only, or unreachable routes.
10. Semantic merging of equivalent Grant Sites or reconverging route branches.
11. Documentation Export, wiki-style Markdown generation, or publication workflows.
12. AI interpretation of raw Mod Source or AI-authored authoritative facts.
13. AI integrations over generated knowledge.
14. Interactive Concept Link popovers.
15. Runtime resolution of saved-game localization tokens.
16. Full emulation of Stellaris tooltip rendering.
17. Continuous source watching and live documentation regeneration.
18. Reliable Paradox launcher enabled/disabled-state detection.
19. Persistent companion accounts, passwords, device registration, or local HTTPS certificate management.
20. Companion-triggered parsing, generation, asset conversion, cache writes, settings changes, or filesystem changes.
21. Protection against an attacker who already controls or can actively intercept the local network.
22. Redistributing third-party Mod Source or assuming that source-visible mods grant open-source licensing rights.

## Further Notes

### Open decisions

1. The project name and copyright-holder wording remain undecided.
2. Parser selection remains provisional until the Jomini real-corpus spike is complete.
3. The representative ordinary drawable vanilla technology for the golden acceptance set will be pinned when implementation begins.
4. The first versioned Resolution Profile remains provisional until the reproducible game-oracle spike establishes each required content-family policy.

### Validated feasibility

1. The local installation contained 976 vanilla technology DDS icons and 1,550 technology DDS icons across installed Workshop mods.
2. Observed assets included uncompressed RGB/RGBA and DXT1, DXT3, and DXT5 variants.
3. Representative vanilla and ACOT DXT5 assets were converted successfully into browser-safe images and visually inspected.
4. Playset composition was confirmed to involve more than uniform file-level last-wins behavior, supporting its separation from the MVP.
5. Tauri can package the desktop experience, but LAN access control and the documentation API require an embedded service rather than a development server.

### Follow-on documents

1. The focused [technical design](./technical-design.md) follows this PRD and resolves module ownership, data shapes, storage, interfaces, and security mechanisms while recording the remaining parser, resolver, asset-artifact, and revision-bundle spikes.
2. An implementation plan should be written only after the technical design is accepted.
3. The [domain glossary](../CONTEXT.md), [decision log](./decision-log.md), [MVP acceptance criteria](./mvp-acceptance.md), [parser spike](./spikes/parser-evaluation.md), [resolver spike](./spikes/resolver-evaluation.md), [DDS spike](./spikes/dds-evaluation.md), [revision-bundle spike](./spikes/revision-bundle-evaluation.md), and ADRs remain the detailed supporting record.
