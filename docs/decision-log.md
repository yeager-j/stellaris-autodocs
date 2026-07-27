# Decision Log

Last updated: 2026-07-26

This log preserves the current product and technical decisions from the design interview. ADRs remain the authoritative record for hard-to-reverse architectural choices; this file also captures reversible product decisions, provisional choices, deferred work, and the next open question.

## Accepted decisions

### Product and audience

**D-001 — Optimize for players**

Player Documentation is the primary product. Mod-author inspection may benefit from source links, but developer documentation is not the organizing use case.

**D-002 — Treat publication as a secondary use case**

Documentation Export, potentially including wiki-style Markdown, is desirable but not required for the MVP.

**D-003 — Organize the product around player questions**

The two leading questions are:

1. How do I unlock this?
2. How do I start this Event Chain?

The model should support many kinds of Unlockable without forcing every kind into the first release.

**D-004 — Start with a technology vertical slice**

Technology is the first deeply supported destination. Technology documentation may still need to inspect events, scripted triggers, scripted effects, and other source constructs that grant, gate, or modify technologies.

**D-005 — Generate generic guidance, not save-aware guidance**

Unlock Paths describe every generic route that can make content available. The MVP will not parse a player's save or attempt to identify their personalized next step.

**D-006 — Prefer structured explanations over prose**

Requirements should retain their logical structure:

- `AND` becomes an “All of” group.
- `OR` becomes an “Any of” group.
- Negated conditions become clearly presented blockers.
- Sequential causality becomes an Unlock Path diagram.
- Numeric modifiers become tables or structured lists.
- Exact source logic remains available in an expandable technical view.
- Unsupported primitives are shown as unsupported rather than guessed.

Short deterministic summaries may assist the reader but are not authoritative.

### Technology documentation

**D-007 — Distinguish eligibility, drawability, and relative weight**

A technology page must separately explain:

- Prerequisite technologies.
- Eligibility requirements.
- Conditions that prevent eligibility.
- Base Draw Weight.
- Every conditional Weight Modifier.
- Direct grants from events or effects.
- Content unlocked after research.

The app cannot calculate an absolute draw probability without live game state, but it can document all generic factors.

**D-008 — Give zero-weight conditions special prominence**

A `×0` Weight Modifier remains mechanically distinct from an eligibility requirement. The page should also summarize it in player language such as “Will not appear in normal research while AI is outlawed.”

Presence in a pool or in the `techweights` console output does not imply that a technology is Drawable.

The pinned Vanilla acceptance example is `tech_combat_computers_3` (Sapient Combat Simulations). Its AI-outlawed modifier has `factor = 0`; the similarly named `tech_sapient_ai` is a prerequisite and does not contain that clause.

### Analysis scope and content model

**D-009 — Document one Target Mod in the MVP**

The MVP analyzes one selected Target Mod against Vanilla Content. Cross-mod references may remain visibly unresolved. A full Playset resolver is a separate future feature.

See [ADR 0002](./adr/0002-resolve-content-before-generating-documentation.md).

**D-010 — Treat Vanilla Content as the base-game file set**

Vanilla Content is the base-game file set, including definitions gated at runtime by DLC ownership. DLC archives do not form a script, localization, interface, or map source layer; `host_has_dlc` is documented as a requirement. Whether DLC archives supply referenced assets is unexercised at the pinned build: all 30 archives under `dlc/` hold only audio, `.asset`, and `.txt` entries, and no image of any format.

Vanilla and Target Mod script pass through the same parser, but source origin is provenance rather than universal precedence. Exact-path and `replace_path` selection run before content-family-specific ordering and registry collision rules.

**D-011 — Resolve content before documentation generation**

The pipeline is:

```text
Mod and vanilla files
    ↓
Parser and indexer
    ↓
Parsed content
    ↓
Content resolver
    ↓
Provenance-preserving resolved content model
    ↓
Documentation generator
    ↓
Player Documentation
```

The documentation generator must not calculate Playset load-order semantics. A future Playset resolver will produce the same resolved model without creating a second documentation path.

**D-012 — Preserve provenance**

Definitions retain their source, including which source introduced or changed them. A future Playset implementation must not flatten ordered mods into merged files and discard override history.

**D-013 — Generate documentation deterministically**

Authoritative facts and relationships come from explicit rules over the resolved content model. AI may later consume the generated knowledge but will not interpret raw Mod Source or establish authoritative facts.

See [ADR 0003](./adr/0003-generate-player-documentation-deterministically.md).

**D-039 — Do not derive documentation from source comments**

Source comments are not executable behavior and do not contribute authoritative facts to Player Documentation. The semantic parser and parsed content model are not required to preserve comments.

The app retains the original raw source and source ranges separately. An expandable technical view may therefore show a bounded Source Excerpt, including comments within that excerpt, without loading or presenting an entire large file.

One captured excerpt is at most 16 KiB of original source, aligned to line boundaries where possible and anchored around the referenced fact. Its display projection represents undecodable bytes visibly rather than dropping them. Visible markers disclose leading or trailing truncation. Provenance retains the complete source range, but an excerpt reference cannot be expanded into arbitrary file access.

**D-040 — Resolve scripted constants into player-meaningful base values**

A scripted constant such as `@acot_tier7cost3` is not meaningful primary documentation. When static source resolution produces one unambiguous value, Player Documentation displays that Resolved Base Value.

The original symbol and its defining source remain available in the technical view. Values affected by difficulty, empire modifiers, saved-game state, missing definitions, or ambiguous resolution are labeled accordingly and never presented as a final player-specific value.

**D-029 — Generate partial documentation when analysis is incomplete**

A malformed file, unsupported construct, or other Analysis Issue does not prevent documentation for successfully analyzed content from being generated. The Target Mod receives a prominent completeness warning, and affected pages or sections receive narrower warnings when the impact can be traced.

Every Analysis Issue identifies its source location and reason where available. The app must not silently omit failed input, invent an interpretation, or present an affected result as complete. Users can still search and browse the usable portion of the Target Mod.

Generated facts retain evidence dependencies. Issues distinguish absent evidence, present-but-unsupported evidence, registry completeness that cannot be established, and downstream facts that explicitly depend on affected evidence. Impact propagates only through those recorded edges: localization failure does not taint mechanics, a scripted-trigger failure affects its consumers, a registry-file failure marks category enumeration incomplete, and an unsupported condition remains visible rather than disappearing.

**D-030 — Analyze complete causality but present linked player-facing sections**

The analyzer retains the complete known causal graph. Player Documentation collapses internal flags, variables, scripted indirection, and bookkeeping events between Player-Facing Anchors. Each inline explanation begins at a useful anchor, shows that anchor's requirements, and links to separate Unlock Paths for prerequisites instead of recursively expanding their complete histories.

An Entry Point is therefore local to a particular explanation, not necessarily the earliest cause reachable from game start. The exact internal chain remains available in the technical view.

**D-031 — Preserve every discovered acquisition route**

An Unlockable may have multiple Unlock Paths, and the documentation preserves every discovered path with its conditions rather than selecting one path as canonical.

For example, Enigmalith can be exposed through the Precursor Databank project route or through a Final Spark origin event route. Both belong on its technology page. Shared terminal effects such as `add_research_option` do not cause their preceding routes to be merged.

**D-032 — Use manual route visibility before automatic classification**

The MVP displays every route discovered in the resolved Target Mod and Vanilla Content without attempting to classify it as player, AI-only, debug, demo, or console-only. Each route exposes enough source and terminal-effect information for the user to assess it.

A user may hide an individual route from the normal page presentation. The page always shows how many routes are hidden and provides Show all and Reset controls. Hiding is a presentation preference only: Hidden Routes remain in generated knowledge and the technical view.

Hidden-route preferences persist per Target Mod using a stable route identity. A materially changed route receives a new identity and becomes visible again rather than inheriting a potentially stale preference.

**D-033 — Keep MVP route guidance concise**

An MVP Route Summary identifies the recognizable activity and required selection that advances the route. For Enigmalith, guidance equivalent to “Complete the Precursor Analysis and select Enigmalith” is sufficient, alongside its direct requirements and resulting unlock effect.

The MVP does not simulate the route's internal progress system or derive cumulative probabilities, expected values, minimum attempts, or success bounds. Literal mechanics remain traceable in the technical view and richer quantitative analysis can be added later.

**D-034 — State each route's precise Unlock Effect**

Every route card explicitly distinguishes what the route does rather than using “unlocks” as a catch-all. Technology Unlock Effects include:

- Adds the technology as a research option.
- Adds a stated amount of research progress.
- Instantly completes the technology.
- Changes the technology's Draw Weight.

A route may have more than one Unlock Effect. For example, the Precursor Databank route adds Enigmalith as an option and adds 20% progress, while the Final Spark route only adds it as an option.

**D-035 — Create one MVP route card per Grant Site**

The parser does not decide when branches are semantically equivalent. For the MVP, each source action that applies one or more Unlock Effects to an Unlockable is a Grant Site and produces one route card.

Unlock Effects co-located in the same enclosing action are combined on that card. Upstream branches remain part of its linked trace but do not independently produce cards unless they contain their own Grant Sites. Separate Grant Sites are not automatically merged, even when they may represent equivalent or reconverging routes.

For example, the Precursor Databank event option that adds Enigmalith as a research option and adds 20% progress produces one card. Its Standard, High, and Extreme allocation branches remain upstream details. A Final Spark event that separately adds the research option produces another card.

The enclosing action is the nearest player action or player-visible occurrence: an event option, event occurrence for `immediate`, project completion, decision, archaeology stage, anomaly option, diplomatic action, or comparable named block. Nested and scripted effects inherit their caller's action unless they introduce another Player-Facing Anchor. The same scripted effect called from two options therefore produces two Grant Sites.

**D-049 — Use five golden cases for MVP acceptance**

The MVP technology vertical slice is accepted against five end-to-end cases:

- A representative ordinary drawable vanilla technology.
- `tech_combat_computers_3` (Sapient Combat Simulations), proving conditional `×0` Draw Weight behavior when AI is outlawed.
- ACOT's Enigmalith, proving multi-category search, multiple Grant Sites, precise Unlock Effects, and linked Route Summaries.
- A deliberately malformed fixture, proving partial documentation and completeness warnings.
- A controlled Vanilla-plus-Target-Mod technology redefinition with omitted `potential`, proving oracle-backed effective fields and provenance.

Generated results for all five cases must also be readable through a Companion-Ready Cache.

See [MVP acceptance](./mvp-acceptance.md).

### Localization and visual assets

**D-014 — Preserve every available language for documented localization**

Ingest the complete resolved localization tables, then preserve every available language for the keys cited by generated documentation plus their Static Localization Reference closure. Unrelated source keys are not retained in the revision. Display the player's configured Stellaris language, fall back to English, then fall back to the raw localization key or script identifier.

See [ADR 0004](./adr/0004-preserve-all-available-localizations.md).

**D-015 — Include technology icons in the MVP**

DDS is not sent directly to the webview. The Desktop Host decodes the locally installed texture, converts it to a browser-safe PNG or WebP, caches the result, and serves it to both the desktop window and Companion Devices. Missing, malformed, or unsupported textures use a clear placeholder without breaking the documentation page.

**D-041 — Render a bounded subset of Stellaris localization markup**

The MVP localization renderer supports:

- Stellaris color and style markers, translated into controlled CSS.
- Recursive Static Localization References, with cycle detection.
- Known inline icons, with readable fallback text when an asset is unavailable.

Runtime Localization Tokens, concept links, formatted runtime values, and unknown constructs remain visibly raw. The renderer does not attempt to emulate live game scope or silently remove syntax it cannot interpret.

### Search and navigation

**D-016 — Make the experience search-first**

After selecting a Target Mod, the primary action is searching for an Unlockable by localized name. Search spans multiple content categories and can be filtered by category, such as technology or megastructure. Browsing by content type remains a secondary discovery path.

Searchable coverage is broader than fully documented coverage. Results lead to the deepest available Player Documentation rather than requiring the user to know which technology gates the content they originally searched for.

**D-036 — Search across content categories**

Search is a unified, multi-category index rather than a technology-only lookup. If a localized term names both a megastructure and its related technology, both appear as separately typed results. Users can filter results by content category.

Technology remains the first category with deep Player Documentation. Other categories may initially be Searchable Entries with their localized name, icon when available, content type, provenance, direct technology gates, and links into the fully documented technology path.

**D-037 — Index four content categories in the MVP**

The initial multi-category search index includes:

- Technologies.
- Megastructures.
- Buildings.
- Ship components.

Technology receives deep Player Documentation. The other three categories may initially use thin Searchable Entries that expose their identity and direct technology gates. Additional Stellaris content categories will be added through the same indexing framework after the MVP.

**D-038 — Search names and identifiers, not body text**

The primary search index matches:

- The localized name in the selected language.
- The English localized name.
- The raw script identifier.
- The raw localization key.
- Partial names and minor spelling errors.

Long descriptions and raw source contents are excluded from primary MVP search to avoid noisy results. Search ranking prioritizes exact and prefix name matches over fuzzy matches and identifier matches.

**D-017 — Exclude Vanilla Content from search by default**

Default search results contain Target Mod additions and modifications. A filter can include Vanilla Content or search all sources. Results show provenance, and a vanilla definition modified by the Target Mod appears as one effective result with both origins.

### Desktop application and setup

**D-018 — Package a local web application with Tauri**

The product is a Tauri desktop application whose native window and authorized Companion Devices use the same responsive React application. Tauri loads the packaged frontend in its window, while an embedded HTTP service provides the frontend and read-only documentation to authorized Companion Devices when Companion Mode is enabled.

See [ADR 0001](./adr/0001-package-a-local-web-app-as-a-desktop-application.md).

**D-050 — Use runtime-specific documentation read adapters**

Shared React features read through one documentation-client interface. Its desktop adapter invokes Tauri commands, while its companion adapter uses HTTP. Runtime bootstrap selects the adapter once; pages do not branch by runtime.

Both adapters use the same response and error shapes, are verified by the same contract suite, and call the same Rust application modules. The Companion HTTP listener runs only while Companion Mode is enabled, so ordinary desktop use does not require a loopback listener, local HTTP credentials, or HTTP bootstrap.

**D-019 — Use a confirmable first-launch setup screen**

On first launch, show a small setup wizard pre-filled with detected Discovery Locations. Display the resulting Mod Library immediately. The user may edit paths or accept the proposal with one Confirm action.

**D-020 — Support multiple Discovery Locations**

The configuration stores multiple detected or manually added Discovery Locations. The UI combines their results into one Mod Library rather than treating any directory as the library itself.

**D-043 — List every discovered installed mod**

The Mod Library includes every mod discovered in Discovery Locations, regardless of whether the Paradox launcher currently enables it. Any installed mod can be selected as the Target Mod.

Launcher enabled/disabled state is not required for the MVP. If a later spike can obtain it reliably, it may be displayed as advisory metadata without changing discovery or selection behavior.

**D-044 — Keep duplicate Mod Installations separate**

Each physical Mod Installation appears as a separate selectable entry, even when another installation has the same title, Workshop identity, or declared version. The Mod Library labels its Discovery Location and path so the user can choose the intended copy.

Installations are not deduplicated automatically because a Workshop copy and local development copy may contain different source. Each has its own content fingerprint and documentation cache.

**D-045 — Treat declared mod dependencies as advisory**

Selecting a Target Mod does not automatically compose its Declared Dependencies. Doing so would introduce partial Playset behavior into the single-mod MVP.

The app displays declared dependencies prominently and warns that the resulting documentation may contain Unresolved References. Dependency metadata may itself be stale or incomplete, so it informs the user without changing analysis scope.

**D-042 — Treat declared game-version compatibility as advisory**

The Mod Library compares a mod's declared `supported_version` with the detected Stellaris version and shows a non-blocking compatibility warning when they do not match.

Declared compatibility may be stale and is not proof that a mod works or fails. It never prevents selection, substitutes for source analysis, or participates in cache identity.

**D-021 — Generate documentation lazily**

The setup scan reads only enough metadata to populate the Mod Library. Vanilla Content is parsed and cached centrally. A Target Mod is parsed when first opened, and cached documentation is reused until its inputs change.

**D-022 — Use content fingerprints for cache identity**

Declared mod versions are display metadata, not cache identity. The authoritative fingerprint incorporates normalized relative file paths and file contents so edits, additions, deletions, and renames are detected.

The documentation cache identity includes:

- Target Mod fingerprint.
- Vanilla Content fingerprint.
- Referenced source-asset identities and byte hashes.
- Parser version.
- Resolver version.
- Documentation-generator version.
- Search-index, localization, and asset-conversion recipe versions that affect published reads.

The complete version vector also names source-enumeration policy, parsed-model schema, Resolution Profile, canonical encoding, Hidden Route identity, and Analysis Issue propagation. The referenced source-asset set comes from the published revision manifest. Ensure re-hashes that set, so an icon edit or deletion invalidates an otherwise unchanged revision; unrelated binary assets remain excluded. Filesystem timestamps and change notifications are performance optimizations rather than proof of identity. A manual refresh remains available.

**D-046 — Do not live-watch source files in the MVP**

Foreground startup performs metadata-only discovery. After the window is usable, one bounded worker verifies only discovered installations that already have published revisions; each is Checking and unavailable to companions until complete. Opening a Target Mod and explicit Refresh verify it immediately, while successful publication supplies a current observation. The MVP does not continuously watch source directories or regenerate documentation immediately after an edit.

Live reload is deferred with the secondary mod-author workflow. Adding it later will trigger the same fingerprint and regeneration pipeline rather than create a second cache policy.

Companion readiness is consequently current as of the latest required verification event, not continuously. Companion HTTP reads consult the latest process-memory observation and never trigger source hashing.

**D-051 — Publish immutable Documentation Revisions atomically**

A completed Target Mod build produces an immutable Documentation Revision tied to exact source fingerprints and analysis-component versions. Build work remains invisible in staging state until its manifest and required cache entries are complete, then publication replaces the readable revision atomically.

Existing reads continue against the revision on which they began. A failed or cancelled build never replaces the previous revision. If its inputs are now stale, the desktop may keep it readable with a prominent warning while rebuilding, but it is not a Companion-Ready Cache.

Incomplete Documentation with disclosed Analysis Issues may still be a completed, publishable revision. It is distinct from a partially written or interrupted build.

**D-052 — Keep build intermediates disposable**

A Documentation Revision persists the generated read model required for search, Player Documentation, causal traces, localization fallback, Analysis Issues, provenance, bounded Source Excerpts, and logical references to required browser-safe assets.

Parser-library output is converted immediately into an application-owned parsed representation. That representation, the resolved content model, Analysis Draft, and typed asset-materialization outcomes are disposable build artifacts: published documentation reads do not require them, and deleting them can cause rebuilding but cannot lose a published revision or user-owned state.

A separately versioned parsed Vanilla Content cache is allowed because it is reused across Target Mods. It remains a disposable performance optimization rather than part of a Documentation Revision. Target Mod intermediates will not be persisted in the MVP unless profiling demonstrates a need.

**D-053 — Store each Documentation Revision as an immutable bundle**

Each Documentation Revision occupies one immutable, application-owned directory containing a versioned manifest, Mod Installation identifier, structured documentation and search data, captured Source Excerpts, and the complete set of required Asset Store keys.

Builds write to an adjacent staging location. After validating the bundle and a content-valid proof for every required asset, the host moves it to its canonical final path and atomically updates the application-state pointer. A crash may therefore leave an unreferenced complete bundle, but cannot expose staging as published documentation.

Bundle completion commits at the durable final move; publication commits at state-path replacement. An ambiguous post-replacement outcome is reconciled by reopening state rather than assuming the old pointer survived. Published bundles are not migrated or modified in place. Revision identifiers and paths do not incorporate untrusted mod names.

**D-054 — Execute search in the Rust host**

React sends a Mod Installation identifier, query, selected language, category and Vanilla Content filters, and a bounded result limit through the documentation client. Application policy resolves the installation into an authorized revision handle before search runs; React never selects a revision.

The host lazily loads per-revision and per-language search material and may retain it in a bounded disposable memory cache. Desktop and Companion Devices therefore receive identical ranking without downloading complete search indexes.

React owns input debouncing, request-generation tracking, superseded-result suppression, and presentation, not matching or ranking. HTTP abort may save transfer work, but host cancellation is not an MVP operation contract and Tauri receives no cooperative token. The specific fuzzy-search algorithm and index representation require real-corpus measurement.

**D-055 — Keep the Rust application in one Cargo package**

The MVP uses the existing executable and library targets in one Cargo package. The executable only starts the application; the library target contains composition, transports, application use cases, and deeper modules.

Tauri and HTTP types remain in thin transport adapters. Parser, image-decoder, filesystem, and persistence types remain in their respective adapters. Application modules communicate through application-owned types and do not depend on transport frameworks.

Additional workspace crates require a real independent consumer or a dependency problem that module organization cannot contain. Crates will not be introduced solely to mirror the module tree.

**D-056 — Derive the Mod Library from Discovery Locations**

The project follows the persistence principle “derive what you can; store what you must.” On startup, lightweight filesystem discovery reconstructs the current Mod Library from Discovery Locations without parsing or fingerprinting complete Mod Source.

The app does not persist a mutable duplicate of the discovered catalog. It may persist Discovery Locations, user-owned preferences, and atomic Documentation Revision references, then reconcile them with the derived installations. Persisted history does not make missing source appear currently installed.

**D-057 — Keep durable mutable state to user intent and publication references**

The app persists Discovery Locations and their corrections; user-owned preferences such as language override, last Target Mod, filters, and Hidden Routes; atomic published-revision references; and the schema version required to read that state.

Mod Library contents, warnings, and cache freshness are derived. Companion Sessions, active build state, and loaded search indexes are process-lifetime or disposable. Stored filesystem metadata may accelerate fingerprinting but cannot authoritatively establish cache identity or freshness.

**D-058 — Derive Mod Installation identity from its Discovery Location**

Each user-confirmed Discovery Location has a stable stored identifier independent of its editable absolute path. Filesystem discovery derives an opaque Mod Installation identifier from that location identifier and the normalized relative mod path.

Content changes preserve installation identity and change the content fingerprint. Titles, declared versions, Workshop identities, and fingerprints are not identity. Moving a mod within a Discovery Location creates a new installation, while the same content discovered through two Discovery Locations remains two installations.

Logical paths use `/`, Unicode NFC, exact case-preserving comparison, normalized UTF-8 byte ordering, and no lossy invalid-Unicode conversion. `.` and `..`, containment escapes, indirection cycles, and normalized-name collisions are rejected. Symlinks, junctions, and reparse points must resolve inside the canonical root but retain their lexical root-relative identity.

Editing a Discovery Location path explicitly rebinds the same location and retains preferences by relative path while revisions are revalidated. The confirmation explains that consequence. A different library is represented by removing the old location and adding a new one.

**D-059 — Store mutable application state as versioned JSON**

One versioned JSON document stores Discovery Locations, user preferences, atomic published-revision references, and its schema version under the application data location. A deep Rust state module owns its schema, loading, in-memory value, serialized mutations, persistence, and recovery errors.

Each mutation encodes a complete next state into an adjacent temporary file, flushes it, atomically replaces the state path, and synchronizes the directory where supported. Replacement is the logical commit point. A pre-replacement failure preserves the prior state; a post-replacement durability failure returns `CommittedDurabilityUncertain`.

After any ambiguous outcome, the module reopens and validates the state path to decide whether memory adopts the next or prior state. If neither can be established, mutation, publication, and garbage collection fail closed into recovery.

Disposable fingerprint accelerators and immutable Documentation Revision bundles remain outside the mutable state file. No React or transport module addresses persistence files directly.

**D-060 — Reset malformed mutable state automatically**

Mutable state is low-stakes and reconstructable. An absent file starts first-launch setup; a supported older schema migrates through the current typed state and atomic persistence path.

Malformed JSON, invalid current-schema state, or a failed older-schema migration is first moved to a timestamped and content-hashed diagnostic quarantine name. Defaults are then persisted, the user is notified, and publication-reference recovery remains unresolved.

While unresolved, orphan bundle and Asset Store garbage collection is suppressed. The focused recovery flow permits restoring the quarantined state or explicitly discarding unrecovered publication references. Every revision manifest records its Mod Installation identifier, and any unreadable retained manifest suppresses deletion because the live set is incomplete.

A newer unsupported schema is preserved rather than reset because it may be valid state from a newer app version. The app stops at a blocking compatibility screen without starting normal use cases, builds, Companion Mode, or mutable defaults that could later overwrite the file. Automatic recovery does not delete immutable Documentation Revision bundles.

**D-061 — Exchange a one-time QR secret for an in-memory Companion Session**

Companion Mode creates a 256-bit cryptographically random, five-minute, single-use pairing secret. The QR URL carries it in the fragment, which the companion bootstrap removes from browser history before sending it in a same-origin pairing request.

The Companion panel always has at most one displayed unconsumed code. Explicit regeneration invalidates the prior code. Fixed-length secret comparison is constant-time. A valid exchange atomically consumes the secret before session creation and immediately rotates the displayed code, allowing another device to pair without invalidating existing sessions. If later exchange work fails, the consumed code is not restored. Invalid submissions do not consume it, but five failures rotate it.

Rust creates a 256-bit random process-memory Companion Session, with at most eight active. The browser receives only a host-only `HttpOnly`, `SameSite=Strict`, `Path=/api/` cookie with no `Domain`; companion JavaScript and local storage do not retain credentials.

Disabling Companion Mode or exiting invalidates all pairing secrets and sessions. Disconnect and stale-session responses explicitly delete the cookie; an idle browser may retain an inert value until it next contacts the host because HTTP cannot push a deletion. No service worker is registered. Because local HTTP cannot use a `Secure` cookie without certificate management, the panel warns that this mechanism gates casual access but does not protect against interception on a hostile network.

**D-062 — Bind Companion Mode on all IPv4 interfaces**

While Companion Mode is enabled, the host binds `0.0.0.0` on an operating-system-assigned port. It advertises a detected private, non-loopback IPv4 address in the QR URL and lets the user choose among plausible addresses without rebinding.

This favors reliable operation across Wi-Fi, Ethernet, and multi-homed hosts. It may also expose the authenticated listener through another active interface such as a VPN; explicit enablement and Companion Session authorization are considered proportionate for the read-only MVP.

The port and address selection are not persisted. The MVP does not add a fixed port, mDNS, UPnP, router port forwarding, or access beyond directly reachable networks.

Binding successfully does not prove another device can reach the listener. Until the first successful pairing request, the desktop panel shows that it is waiting for a connection and offers troubleshooting for the same network, guest or client isolation, firewall permission, and alternate advertised addresses.

**D-063 — Resolve companion authorization into a trusted revision handle**

The Companion HTTP adapter extracts a presented credential, but the companion access module authenticates it, consults the latest process-memory freshness observation for the published manifest, selects the eligible revision, and opens a request-scoped companion revision handle. The observation is updated by bounded post-startup verification of published revisions, Target Mod open, Refresh, or successful publication; a documentation request never hashes source.

Only that module constructs the handle. Search, documentation, asset, localization, and Source Excerpt readers accept it instead of raw client revision IDs, paths, or an `is_companion` flag. The desktop constructs a distinct handle that may read a stale published revision with its warning state.

Both handles use the same immutable revision reader after access policy has been decided. Each pins its bundle for one operation, and retention cleanup cannot delete a referenced bundle. If Refresh discovers staleness, an active request finishes against its pinned revision and subsequent requests for that mod receive `DocumentationUnavailable` until a matching revision is published.

**D-064 — Keep the Companion HTTP service strictly same-origin**

The embedded service provides the React app, pairing, authenticated documentation reads, and assets from one origin. Production sends no CORS permission headers. Exactly one `Host` is parsed as an RFC 9112 authority and must equal an advertised IPv4 literal plus active port; malformed, duplicate, substring, user-information, and omitted-port forms fail. Pairing also requires an exactly matching parsed HTTP `Origin`.

Framework-level limits bound headers, request target, pairing body, connections, concurrency, header and body receipt, application duration, and idle time. Pairing, session, JSON documentation, and Source Excerpt responses use `private, no-store`; content-hashed assets use `private, max-age=31536000, immutable`.

Production applies separately tested desktop and companion CSPs, MIME-sniffing protection, and a no-referrer policy. Source content is never raw HTML, `dangerouslySetInnerHTML`, remote resources, `unsafe-eval`, and service workers are excluded, and any diagram-required inline style exception is isolated and justified.

Development uses a Vite proxy. A debug-only exact-origin allowance is permitted only if the proxy is inadequate. Production CORS requires a real separate client and a new security review.

**D-065 — Give one process ownership of an application-data directory**

The packaged application allows one Desktop Host process per fixed application identifier. Its fixed identifier-to-data-directory mapping makes that process the packaged authority for mutable state, builds, revision publication and cleanup, Companion Mode, pairing secrets, and Companion Sessions.

A subsequent normal launch activates the existing window rather than constructing another host. Multiple windows or tabs may exist within that process. Concurrent development or test processes require explicitly separate temporary data directories.

Ownership uses the official single-instance plugin. Its Windows and Linux coordination is process-released; its macOS Unix-socket implementation connect-tests and removes stale sockets after connection refusal. The plugin does not lock arbitrary data roots, so differently identified development binaries must never share a directory.

**D-066 — Retain only the current published Documentation Revision**

Each Mod Installation has at most one published revision. Its existing revision remains readable during a replacement build; publication atomically selects the validated replacement and makes the superseded bundle eligible for deletion.

Active revision handles pin their bundle until the operation completes. Cleanup runs only after state recovery is resolved and every retained manifest has produced a complete revision and asset live set. An unreadable manifest suppresses deletion. Failed and cancelled builds preserve the current bundle.

The MVP has no revision history, rollback UI, history count, or history quota. Revisions are deterministic rebuildable caches rather than user-authored data.

**D-067 — Retain state while a Discovery Location is unavailable**

Temporary filesystem unavailability preserves the Discovery Location, preferences, publication references, and revision bundles. The desktop reports the unavailable location and retries discovery later, but its undiscoverable mods are neither currently installed nor Companion-Ready.

Explicitly removing a Discovery Location is a confirmed intent operation that removes its configuration, per-installation preferences, and publication references. Its bundles are deleted after active handles release them.

The MVP has no general cache-management screen. Refresh and rebuild replace the current revision, while automatic cleanup handles staging, superseded, explicitly removed, and otherwise unreferenced bundles.

**D-068 — Share converted assets by content and conversion recipe**

Browser-safe assets live in one immutable, application-owned Asset Store rather than being copied into Documentation Revision bundles. A revision records logical references and its complete required-key set.

Each asset key derives from the source bytes, an explicit conversion-recipe version, output format, and conversion parameters. Unchanged inputs reuse the existing blob across rebuilds, mods, and Vanilla Content; changed bytes or behavior produce a new key.

Publication verifies every required blob through content hashing or atomically recorded trusted metadata; path existence is insufficient. Asset resolution requires an authorized revision handle and confirms membership before returning a transport-specific descriptor. Garbage collection derives live keys from every readable retained manifest and fails conservatively when that set is incomplete. Runtime cleanup applies a delivery grace period.

This makes revisions logically complete but not physically self-contained. Documentation Export must copy its referenced blobs into the exported artifact.

**D-069 — Deliver desktop images through Tauri's scoped asset protocol**

Rust resolves a logical asset reference against a desktop revision handle and returns its Asset Store path after membership validation. The Tauri documentation adapter converts that path into an asset-protocol URL; React receives only the resulting browser URL.

The asset-protocol scope is the platform-resolved equivalent of `$APPDATA/asset-store/**`, and the desktop CSP permits it only in `img-src`. Revision JSON, Source Excerpts, mutable state, Discovery Locations, and Mod Source remain outside the scope.

Companion Devices use an authenticated same-origin HTTP asset route over the same logical references. A custom Tauri URI protocol and IPC binary image transfer are deferred fallbacks, not MVP defaults.

**D-070 — Separate expected Results from transport failures**

A valid HTTP application operation returns `200` with the shared Result envelope even when the expected outcome is `ok: false`. Malformed requests use `400`, Companion Session failures use `401` or `403`, unknown HTTP routes use `404`, and unexpected internal failures use an opaque `500` with a correlation identifier.

Tauri commands resolve with Result for expected outcomes and reject opaquely for malformed invocation, framework failure, or unexpected internal failure. Both TypeScript adapters normalize those transport failures into thrown errors rather than synthesizing Result errors from statuses or rejections.

Unexpected internal failures use a typed Rust error channel, not panic-based control flow. Detailed chains stay in protected desktop logs with credentials, source content, and absolute paths redacted. React route and application error boundaries present only safe context and the correlation identifier.

**D-071 — Decode Result envelopes without duplicating every response schema**

Each TypeScript transport adapter checks that a Rust response is a plain Result envelope with a boolean discriminant and exactly the corresponding `value` or `error` property. Malformed envelopes throw a transport-contract error.

The frontend does not maintain full duplicate runtime schemas for every Rust DTO. Same-version packaging, Rust types, persisted-revision validation, and cross-language contract tests provide the primary guarantees. If drift becomes a demonstrated problem, prefer generated contracts over hand-maintained parallel schemas.

This does not weaken parsing at genuine external-input boundaries; it applies only to responses produced by the same packaged application version.

**D-072 — Keep companion route visibility overrides session-local**

Documentation Revisions retain every route, while persistent Hidden Route identities remain desktop-owned mutable preferences. Desktop controls may hide, restore, and reset those preferences.

Companion reads receive all routes and the persisted hidden identities. The browser may temporarily Show all, hide, or restore routes in memory for the current Companion Session, but it does not mutate the host or use local storage.

Reloading or pairing again discards companion overrides. Shared route components receive a visibility capability so only the desktop exposes persistent mutation controls.

**D-073 — Address entries by category and script identifier**

An Entry Key is the pair of a content category and its raw Stellaris script identifier, such as `technology + tech_combat_computers_3`. The identifier is stable across localization changes and already serves as the source-language reference target; the category prevents collisions across distinct content registries.

Redefinitions with the same Entry Key are one logical Searchable Entry rather than conflicting documents. Resolution produces the effective entry and retains provenance for contributing or shadowed definitions. The exact override behavior remains content-type-specific and must be reproduced by the resolver rather than inferred from the identity model.

**D-074 — Keep Documentation Revision identity out of entry addresses**

Frontend entry addresses use a Mod Installation identifier and Entry Key. They do not contain a Documentation Revision identifier, fingerprint, localized name, mod title, or bundle path.

The host resolves the installation's currently published revision under desktop or companion access policy. Rebuilding therefore preserves bookmarks, while missing installations and entries remain expected application outcomes. Revision identifiers stay internal to cache ownership and diagnostics rather than becoming browser-held authority.

**D-075 — Build a Vite SPA with TanStack Router**

The shared React frontend is a client-rendered single-page application built as static assets by Vite. The MVP does not run Next.js, Node.js, React server rendering, or another frontend server at runtime; the Rust process remains the sole application host.

TanStack Router owns URL matching, typed path and search parameters, nested page composition, and navigation state. Route loaders may coordinate documentation-client reads, but revision selection, cache freshness, generated documentation, and companion authorization remain host-owned.

SSR adds a frontend-server runtime, lifecycle, packaging path, cache, and authorization integration without providing meaningful SEO or network-latency benefits to a local desktop and authenticated LAN application. A Next.js static export would not provide SSR and offers no current advantage over the existing Vite build.

**D-076 — Use browser-history routes**

TanStack Router uses browser-history paths rather than hash history. Entry addresses remain ordinary shareable paths, while the URL fragment remains available for the one-time Companion pairing secret and is removed during bootstrap.

The Companion HTTP service returns the packaged React shell for direct requests to recognized frontend paths without conflating them with API, asset, or unknown reserved paths. Tauri's built-in asset resolver supplies packaged `index.html` for unknown frontend paths. Packaged Tauri and Companion builds prove those mechanisms preserve refreshes, bookmarks, and direct navigation.

**D-077 — Do not add TanStack Query for the MVP**

TanStack Router loaders coordinate route-level documentation reads. The router uses an explicitly configured finite `gcTime` and immediate or short `staleTime` rather than relying on its default 30-minute garbage-collection window. Interactive search suppresses responses from superseded request generations; HTTP abort is an optimization, while Tauri and the host operation have no MVP cancellation token.

The Rust host remains authoritative for revisions, freshness, documentation, and Mod Library status. Desktop operations that change relevant host state explicitly invalidate affected router matches. Components keep only presentation-local state rather than maintaining a second normalized copy of host data.

TanStack Query or another general request cache requires evidence of cross-route deduplication, polling, optimistic mutation, or invalidation complexity that the router lifecycle cannot contain.

**D-078 — Make search a navigation combobox, not a results page**

The selected Mod Installation's landing page and shared documentation layout expose a lazy-loading search combobox. It returns bounded, categorized results and navigates directly to the stable address of the selected entry.

Query text and category and Vanilla Content filters are session-local presentation state rather than URL parameters. Back navigation restores them during the current frontend session. Narrow screens may present the same control as a full-screen dialog without introducing another route.

A dedicated results page is deferred until actual use requires large-result exploration, advanced faceting, comparison, or shareable queries.

**D-079 — Shape documentation reads around product use cases**

The shared documentation client exposes a small set of reads for the runtime-visible Mod Library, bounded entry search, one documented entry, revision-level Analysis Issues, one bounded Source Excerpt, and resolution of one logical asset reference.

Inputs and outputs use Mod Installation identifiers, Entry Keys, opaque excerpt and asset references, and application-owned DTOs. React does not receive revision identifiers, bundle paths, JSON filenames, absolute source paths, arbitrary filesystem access, or a generic query language.

Full entry reads contain the completeness, provenance, and logical references needed to render the page rather than forcing React to reconstruct a document through low-level calls. Both transports implement this same client boundary over the same Rust application use cases.

**D-080 — Expose explicit Companion HTTP resources**

The Companion HTTP adapter exposes separate resource-oriented endpoints for the Mod Library, entry search, documented entries, revision Analysis Issues, bounded Source Excerpts, assets, and pairing. It does not introduce JSON-RPC or a generic query endpoint.

Search is a safe, idempotent `GET` whose bounded query parameters carry query text, repeated categories, Vanilla Content inclusion, language, and result limit. This request URL is independent of the combobox's session-local frontend-route state.

JSON documentation reads use the shared Result envelope. Authenticated asset `GET` requests return binary content, and pairing remains a state-changing `POST`. Tauri commands use equivalent DTOs but call the same Rust application use cases directly rather than invoking HTTP.

**D-081 — Give each operation a narrow Result error union**

Every documentation-client operation advertises only the expected error variants it can produce. Shared variants may reuse the same serialized DTO, but the frontend does not receive one global application-error union containing unrelated outcomes.

Normal product states such as Needs build, Out of date, Incomplete Documentation, Analysis Issues, and empty search results remain successful data where the interface presents them. Authorization, malformed transport input, programmer errors, corrupted internal artifacts, and unexpected failures remain transport or thrown failures.

The exact fields of each operation-specific union are defined alongside its application use case and contract tests rather than speculated independently.

**D-082 — Make analysis one deep build module**

The `analysis` module accepts exact Target Mod and Vanilla Content Source Snapshots and first produces an unpublishable Analysis Draft with logical asset slots. `assets` returns one typed materialization outcome per slot. `analysis::finalize` owns placeholder substitution, scoped Analysis Issues, and the final required-key set, then produces a Revision Candidate.

Parser, resolver, generator, and indexing intermediates do not escape into application use cases or transports. Their internal modules retain direct domain tests, and the parser adapter earns a substitution seam because Jomini is an external dependency. The corpus spike is complete: Jomini's token tape does lack structural-token source ranges, but its `TokenReader` lexer supplies byte positions for every token, so the adapter wraps the lexer and the tape is not used. See [ADR 0007](./adr/0007-parse-stellaris-source-through-a-wrapped-incremental-lexer.md).

The `revisions` module consumes the finalized candidate plus a sealed content-valid Asset Store proof and owns bundle writing, validation, atomic publication, opening, and retention. Recoverable source problems become disclosed Analysis Issues; a missing or duplicate asset outcome or fatal inability to establish candidate identity or integrity produces no candidate.

**D-083 — Expose build intentions, not pipeline stages**

Desktop application use cases distinguish Ensure Documentation, Rebuild Documentation, Validate Published Revision, and a possible future debug-only recovery operation. Ensure may return a valid cache hit; Rebuild deliberately bypasses that hit; Validate is read-only.

Ensure and Rebuild share one private coordinator with an explicit cache policy. Both establish source identity, produce a draft, materialize assets, finalize the candidate, obtain content-valid asset proof, and publish atomically. Cache bypass never bypasses fingerprints, finalization, validation, staging, or revision publication rules.

A cache hit means analysis, conversion, and publication were skipped after freshness verification. It may still require seconds of filesystem enumeration and hashing, so the desktop presents an explicit Checking for changes phase rather than promising instantaneous cache access.

Parse, Resolve, Generate, and unchecked Publish are not independently sequenced frontend commands. Purpose-built diagnostics may inspect internal stages without making the UI responsible for pipeline invariants. Companion Devices receive no build or diagnostic capability.

**D-084 — Verify the source snapshot again before publication**

A build enumerates analysis-relevant files deterministically, then hashes and parses the same bytes. The resulting Revision Candidate is tied to that exact logical source snapshot.

Immediately before publication, the host recomputes the authoritative live-source fingerprint. If paths or contents changed during analysis, it discards the candidate and preserves the previous published revision. Timestamps and metadata may accelerate work but cannot replace this content check.

The initial implementation may read source twice for correctness. Referenced assets participate through normalized source identities and source-byte hashes as well as content-derived Asset Store keys and final validation. The manifest retains the reference set needed for the next freshness check; unrelated binary assets do not need to be included merely because they share the mod directory.

**D-085 — Give source traversal and identity one module**

The `source` module owns deterministic relevant-file enumeration, logical path normalization, exact-byte reading and hashing, Target Mod and Vanilla Content fingerprints, build-lifetime Source Snapshots, final consistency verification, and disposable metadata accelerators.

`discovery` remains a lightweight installation and metadata scan. `analysis` receives app-owned Source Snapshots rather than installation paths or arbitrary filesystem access, so neither module duplicates filesystem identity rules.

Analysis integration tests construct snapshots from realistic fixture corpora through source-owned test support. The source module separately proves edits, additions, deletions, renames, ordering, path rejection, and mid-build changes. Snapshot storage remains an internal implementation choice subject to measurement.

**D-086 — Give search indexing and querying one owner**

The deep `search` module owns deterministic index construction, its versioned application-owned representation, encoding and decoding rules, query normalization and matching, filtering, ranking, and bounded result selection.

`analysis` invokes index construction as an internal build stage, while runtime documentation reads receive a validated decoded index from the Revision Reader and query it through the same module. The application layer and transports do not implement parallel indexing or ranking rules.

This keeps the persisted index contract and the code that interprets it together, while allowing loaded indexes to remain disposable bounded memory caches.

**D-087 — Make revisions the sole bundle I/O owner**

The `revisions` module exclusively writes, validates, opens, reads, pins, and retains Documentation Revision bundles. An authorized handle provides a typed Revision Reader for entries, Analysis Issues, Source Excerpts, localization data, and decoded search indexes without exposing paths or JSON filenames.

Application use cases, search, transports, and React do not open bundle files. Search owns its index representation and algorithms, while the Revision Reader owns physical loading and schema validation. The Asset Store remains separately owned by `assets`.

This boundary contains materialized-JSON persistence and allows a later SQLite-backed Revision Reader without changing product reads or transport contracts.

**D-088 — Separate asset semantics from asset mechanics**

Analysis resolves which effective Stellaris asset belongs to a documented concept, including icon conventions, sprite definitions, source precedence, and provenance. The Analysis Draft emits a slot identifier, resolved Source Asset Reference, and conversion recipe.

The `assets` module reads that reference through the Source Snapshot capability and owns content hashing, DDS or other supported decoding, conversion, deduplication, atomic Asset Store publication, trusted blob metadata, content validation, membership checks, and delivery. It returns typed success or failure outcomes and does not interpret technologies, content overrides, localization markup, placeholders, issue scope, or sprite-selection rules.

`analysis::finalize` turns missing semantic references and byte or conversion failures into disclosed scoped Analysis Issues and deterministic placeholders. Revision publication accepts only keys covered by a content-valid proof; a corrupt blob at the expected path cannot pass.

**D-089 — Give localization one parser and resolver**

The deep `localization` module owns localization-file ingestion, markup tokenization, language fallback, Static Localization Reference resolution with cycle detection, preservation of runtime and unknown constructs, plain-text search projection, and safe display-token production.

Analysis preserves every language and parsed localization structure in the Revision Candidate. Search indexing uses the same module's plain-text interpretation. At read time, the module selects a language from revision data and returns display tokens; React renders those tokens through controlled components and CSS without parsing Stellaris localization syntax.

Language changes query the same immutable revision and do not require source parsing or documentation regeneration.

**D-090 — Let revisions own publication sequencing**

The `revisions` module owns candidate validation, bundle finalization, atomic publication-reference replacement, and retirement eligibility. Application use cases supply the finalized candidate and exact Asset Store proof to one publication operation rather than manually sequencing filesystem and state changes.

The `state` module retains ownership of JSON persistence and exposes only a narrow atomic publication-reference mutation. `revisions` cannot modify unrelated settings or access the mutable state representation.

Bundle completion commits at the durable final move; publication commits at atomic state-path replacement. After an ambiguous pointer outcome, state is reopened and validated to establish which revision is authoritative. The previous bundle becomes eligible for retirement only after the replacement pointer is observed committed; unresolved state or an unreadable retained manifest suppresses garbage collection.

**D-091 — Allow one active documentation build**

Ensure Documentation and Rebuild Documentation share one process-wide mutating-build lease held through snapshot creation, draft analysis, asset materialization, candidate finalization, validation, publication or failure cleanup. A concurrent request returns an expected `BuildInProgress` result identifying the active Mod Installation rather than joining, queuing, cancelling, or starting another build.

Published documentation reads and read-only revision validation remain available through immutable handles. The one active build may still use bounded internal parallelism.

Multiple builds, a persistent queue, and Build all are deferred until measured duration and user demand justify their resource and interaction policies.

**D-092 — Use Tauri's official single-instance plugin**

The packaged desktop app registers `tauri-plugin-single-instance` before every other Tauri plugin. A second launch notifies the first process, whose callback shows, restores, and focuses the existing main window; the second process does not construct another application host.

Windows mutex and Linux DBus ownership are released with the process. On macOS, the current plugin uses an application-identifier-derived Unix socket under shared `/tmp`; a new launch connect-tests and removes a stale socket after connection refusal. Ordinary stale-socket recovery is covered by a packaged test, while possible cross-user collision is an accepted upstream MVP limitation. Snap and Flatpak packages require explicit DBus permissions and release verification.

The invariant is one packaged process per application identifier, extended to its data directory only by the packaged fixed identifier-to-directory mapping. Concurrent automated or development processes use isolated application-data directories and disable the packaged plugin or use distinct identifiers; the plugin does not protect arbitrary custom roots.

**D-093 — Use a domain-oriented top-level Rust module map**

The one Cargo package contains top-level `composition`, `transport`, `application`, `discovery`, `source`, `analysis`, `localization`, `search`, `revisions`, `assets`, `state`, and `companion` modules. Tauri and HTTP are sibling transport adapters.

`application` owns named product use cases and cross-module coordination without becoming a generic service or repository layer. `composition` constructs concrete dependencies and process-lifetime resources. `companion` owns pairing, sessions, listener lifecycle, advertised addresses, companion access policy, and trusted companion revision handles.

Parser adaptation, content-type-specific resolution, documentation generation, and Source Excerpt capture remain internal to `analysis`. The other deep modules retain their separately decided authorities, and transport or framework types do not cross into them.

The sanctioned direct deep-module edges are `analysis -> localization`, `analysis -> search`, `assets -> source`, `revisions -> state` through the narrow publication capability, and `companion -> revisions`. Companion freshness observations arrive from application workflows after source verification; companion reads do not call source hashing. Additional edges require explicit ownership and a cycle check.

**D-094 — Verify through fixture builds and published reads**

The primary acceptance harness sends all five controlled golden corpora through draft analysis, asset finalization, the real publication use case, typed desktop reads, and authorized Companion HTTP reads.

Golden cases assert semantic facts, identities, relationships, completeness, provenance, and user-visible DTOs rather than snapshotting complete bundles, JSON files, parser trees, or rendered pages.

Focused suites cover canonical reproducibility, metamorphic source edits, oracle-backed resolver behavior, exact numerics, completeness propagation, asset outcomes, injected ambiguous persistence failures, conservative garbage collection, status derivation, security and path boundaries, every transport union variant, and responsive React behavior.

The selected release formats are macOS `.dmg`, Windows NSIS, Linux AppImage, and Debian `.deb`. Real packaged tests cover replacement semantics, exact asset scope, browser-history fallback, Companion firewall behavior, and single-instance behavior on the platforms for which each format claims support.

**D-095 — Treat actionable build failures as expected Results**

Ensure and Rebuild use narrow expected variants for Installation Unavailable, Build In Progress, Source Changed During Build, Source Unavailable, Analysis Failed without a publishable candidate, and Storage Unavailable. Variants are included only in operations that can produce them and carry stable actionable reason data.

Recoverable source problems still publish Incomplete Documentation with Analysis Issues. Validate Published Revision successfully returns a validation report when it discovers corruption.

A newly generated bundle failing its own validation, an impossible state transition, a corrupted internal contract, or a programmer defect remains an unexpected typed internal failure rather than a routine Result error. Panic is not an application error channel. Transports return opaque failures with correlation identifiers, protected logs redact credentials, source contents, and absolute paths, and React error boundaries present only safe context.

**D-096 — Keep companion language overrides session-local**

Desktop language selection is persisted user configuration. Each new Companion Session starts with that selection so documentation initially matches the terminology used in the player's game.

A companion may select another available language in browser memory for its current session. Search and entry reads use that effective language, but the override does not mutate host configuration or use local storage. Reloading or pairing again restores the current desktop selection.

**D-097 — Derive the effective desktop language**

The effective desktop language is an explicit app override, otherwise the currently detected Stellaris language, otherwise English. Only the explicit override is persisted as user intent.

The game language is derived from current Stellaris configuration during startup and Refresh. Without an override, changing the game's language changes the documentation language automatically. Missing revision or key localization still falls back independently to English and then the raw key.

If macOS Documents-folder privacy prevents reading game settings, the app reports the access limitation without blocking documentation, then uses the explicit override or English. First-launch discovery similarly presents an affected local-mod location as unavailable and offers folder selection or path correction plus retry; denial never masquerades as an empty library or deletes configuration.

**D-098 — Require an oracle-backed Resolution Profile**

The MVP resolver consumes a versioned policy matrix for exact-path shadowing, directory replacement, content-family file streams, and each supported registry. It receives two source contributors—Vanilla Content and one Target Mod—but does not treat them as universally ordered layers. Script and sprite content use global logical-path order across surviving files; localization uses its own Vanilla, enabled-mod, and `replace/` stream. DLC ownership is a requirement fact, not a definition-source layer.

Every row defines keys, semantic stream construction, duplicate and cross-source collisions, field replacement or inheritance, defaults, ordering, unresolved references, and contributed, inherited, defaulted, duplicate, and shadowed provenance. The [resolver evaluation](./spikes/resolver-evaluation.md) completed all evidence collection possible before the resolver exists. Its captured records define golden expectations for resolved cells. A content type is supported only when every policy it requires is explicit and oracle-backed; unresolved cells fail visibly without blocking implementation of unrelated resolved rows.

**D-102 — Wrap Jomini's incremental lexer rather than its token tape**

Jomini is an accepted dependency, consumed through `TokenReader` and adapted into the application-owned parsed representation. The tape is not used: it exposes no byte position for a brace or an operator, so a Source Excerpt cannot be bounded to a definition; it rejects real vanilla and mod files using escaped constant arithmetic, bare token lists, and conditional-compilation blocks; and it accepts unbalanced braces, silently reparenting the remainder of the file.

The production adapter accepts exact bytes and a logical source identity and returns an application-owned `ParsedFile`. Definitions and nodes retain order, exact byte ranges, and Clean or Recovered evidence quality; faults retain byte positions and recovery boundaries. The adapter resynchronizes past a syntax fault and lexes the Stellaris dialect constructs Jomini does not recognize. Every range is verified by re-slicing it from the source across the whole corpus rather than across fixtures.

Recovery is a layout heuristic. Any definition the parser cannot emit at a fault is absent, definitions before the first fault remain Clean, and definitions emitted after resynchronization are Recovered. `analysis` emits a file-scoped recovery issue while propagating incompleteness only from absent and Recovered evidence, preserving unaffected Clean facts. The [parser evaluation](./spikes/parser-evaluation.md) holds the measurements and residual limitations; [ADR 0007](./adr/0007-parse-stellaris-source-through-a-wrapped-incremental-lexer.md) records the decision.

**D-099 — Canonicalize every identity input**

Paths, source enumeration, registries, provenance, maps, sets, semantic sequences, requirements, modifiers, routes, issues, excerpts, search normalization, manifests, and revision identity use versioned application-owned canonical encodings and total orders. Temporary roots, timestamps, worker schedules, JSON object order, and absolute paths do not participate.

Revision identity is a SHA-256 digest of a domain separator and canonical manifest body. The analysis version vector includes source enumeration, parsed-model schema, Resolution Profile, documentation, localization, search, canonical encoding, Hidden Route identity, issue propagation, and asset recipes.

Numeric literals preserve their source lexeme and use exact arbitrary-precision rational normalization for supported static arithmetic. Binary floating point does not feed source equality, identity, hashes, or exact Base Values; unproven game rounding remains unresolved.

**D-100 — Derive Hidden Route identity from the enclosing action**

The enclosing action is the nearest player action or visible occurrence. Nested and scripted effects inherit their caller unless they introduce another Player-Facing Anchor. Effects for the same destination under one action produce one Grant Site; two call sites remain distinct.

Hidden Route identity hashes the destination Entry Key, named enclosing action, canonical anchor, direct requirements and selections, normalized Unlock Effects, and call-site structure. Comments, whitespace, ranges, absolute paths, localization text values, and unrelated siblings are excluded. Identical sibling collisions receive an occurrence ordinal among only identical structures.

**D-101 — Derive documentation status from orthogonal facts**

Availability, published-artifact presence, freshness, integrity, completeness, build state, and desktop-versus-companion context remain separate facts. Foreground startup is metadata-only; a bounded worker verifies only installations with published revisions. Each is Checking and closed to companion access until current.

Desktop reads may use a valid unchecked, checking, stale, or incomplete revision with warnings. Companion access requires available source, current freshness, valid integrity, resolved state recovery, and a completed revision; disclosed Incomplete Documentation remains readable.

### Companion experience

**D-023 — Make LAN companion access a supported capability**

The responsive application is available from another device on the Desktop Host's local network without a separate mobile app. Companion Mode is an explicit product capability rather than an accidental development-server behavior.

**D-024 — Keep Companion Devices read-only**

Companion Devices may search, browse, filter, follow Unlock Paths, and switch among Companion-Ready Caches. They may not change Discovery Locations, configuration, caches, app settings, or filesystem state. Privileged operations remain exclusive to the Desktop Host.

**D-028 — Gate Companion Mode with temporary QR-code sessions**

Companion Mode is off by default. Enabling it generates a random temporary secret and displays a QR code containing the local address and secret. Opening that link exchanges the secret for a Companion Session, which is required for subsequent documentation API requests.

Disabling Companion Mode or exiting the desktop app invalidates its secrets and sessions. The MVP will not introduce accounts, passwords, persistent device registration, or local HTTPS certificate management. This is a proportionate access gate for read-only documentation; it does not claim to prevent interception by an attacker already controlling the local network.

**D-047 — Let companions browse only validated caches**

The Companion Mod Library exposes every discovered Mod Installation and its documentation status:

- **Ready** — a Companion-Ready Cache can be opened immediately.
- **Needs build** — documentation must first be generated on the Desktop Host.
- **Checking** — a published revision is being verified after startup and remains closed to companion reads.
- **Out of date** — cached documentation no longer matches its inputs and must be rebuilt on the Desktop Host.

A companion may switch among Ready mods without changing the desktop's active Target Mod. Selecting a missing, stale, or unverified cache does not trigger parsing, generation, asset conversion, or filesystem writes; the companion instead instructs the user to build it on the computer.

**D-048 — Allow bounded Source Excerpts on Companion Devices**

Companion Devices may open the same bounded Source Excerpts used by the desktop technical view. Mod source is locally readable content distributed with the Mod Installation, so hiding these excerpts provides little product value.

The companion API exposes only mod-relative source paths and cached excerpt content. It never returns absolute host paths, provides arbitrary filesystem access, or exposes desktop-only actions such as opening an editor or revealing a file.

This decision treats mod source as source-visible; it does not assume every mod grants an open-source redistribution license.

### Delivery and licensing

**D-025 — Develop the functional MVP on macOS**

The technology vertical slice may be developed and validated on macOS first. The architecture must remain platform-aware rather than macOS-specific.

**D-026 — Test all desktop platforms before public release**

The selected first-release formats are macOS `.dmg`, Windows NSIS, and Linux AppImage plus Debian `.deb`. Each is tested on a real machine for replacement semantics, single-instance behavior, asset scope, browser-history refresh, and Companion firewall behavior before release. RPM, Flatpak, Snap, and other formats remain unclaimed until separately tested.

See [ADR 0005](./adr/0005-develop-on-macos-with-a-three-platform-release-target.md).

**D-027 — Release the project as open source under MIT**

Required dependencies must not introduce reciprocal distribution obligations without a separate explicit decision.

See [ADR 0006](./adr/0006-license-the-project-under-mit.md).

**D-103 — Store the revision read model as self-contained per-document JSON**

A Documentation Revision is a compiled set of denormalized JSON read models with one file per document, not a relational database and not a sharded one. Every view is materialized from one canonical in-memory documentation model during the same build.

A revision preserves the localization its documentation cites, plus the closure of that set's static references, in every available language. That is 1.15% to 1.45% of preserved localization on the measured slice — 1.74 to 2.59 MiB against 151 to 178 MiB. Following the closure is required rather than defensive: roughly one documented text in ten embeds a static reference, and preserving only directly named keys would render a raw placeholder mid-sentence.

Build-time search material covers the selected language and English, which requirements 21 and 28 name as separate index inputs. If the user later selects another available language, the same search module derives that language's index in memory from the immutable revision and may retain it in the bounded disposable cache. This neither reparses source nor mutates or rebuilds the revision.

The pre-authorized shared Localization Store is not built. It would deduplicate keys no reader reads, and it cost a module, chunk-key manifests, cross-store garbage collection, and roughly half of every build. Sharding was measured and rejected against a file count one seventh of budget. SQLite was measured and not adopted, because no budget failure had a cause the rule assigns to it.

Localization is not resolved from live source at read time. That would save two megabytes and cost the self-contained immutable artifact the rest of the design rests on.

React never addresses bundle files directly. Rust serves product-level reads through the documentation-client transports, so a later internal move to SQLite would still not change the frontend or Companion HTTP representation.

See [ADR 0009](./adr/0009-store-revisions-as-self-contained-per-document-json.md) and [Revision bundle evaluation](./spikes/revision-bundle-evaluation.md).

**D-104 — Run documentation builds as an awaited asynchronous Tauri command**

Measured p95 complete builds are 1.8 to 2.3 seconds for every representative Target Mod, against the declared three-second threshold, and navigation does not need to outlive the invocation. Build work still runs outside the UI and asynchronous I/O execution paths, writes to private staging state, and uses atomic revision publication.

The threshold was missed only while the build chunked 1.5 million localization keys per revision for a shared store that is no longer built. With that removed, the dominant cost is the correctness-first double fingerprint at 50–62% combined, followed by resolution. An explicit host-owned job remains the answer if a deeper generator approaches the threshold again; the single-build-lease rule is independent of either choice.

### Implementation foundations

Recorded on completion of Phase 0 (2026-07-26). See [the Phase 0 plan](./plans/phase-0-foundations.md).

**D-105 — Place shared primitives in `canonical` and `error` leaf modules**

The technical design's module map names the deep-module row and its adapters; it does not name a home for primitives every one of those modules consumes. `canonical` (domain-separated digest encoding, logical paths, exact numerics) and `error` (correlation identifiers, the expected/unexpected failure channel) are top-level leaf modules that sit *below* that row rather than beside it.

Only encoding mechanics and error conventions are shared. Each identity's field order and schema stay owned by the module that defines that identity, so `canonical` cannot become a second authority over what any identity means.

**D-106 — Pin the Rust toolchain at 1.97.1 and adopt edition 2024**

`rust-toolchain.toml` pins the channel and the `rustfmt`/`clippy` components so the local gate and CI compile identically. The edition was bumped from the scaffold's 2021 to 2024 while the crate was still empty, which is the only moment the change costs nothing.

**D-107 — Use the `test-support` cargo feature as the standing product test seam**

Test-only helpers live behind an off-by-default feature, enabled for this package's own tests through a self dev-dependency. A production build never enables it, so a test seam cannot reach a shipped binary. `testsupport::TempAppData` is the first member: an isolated, disposable application-data directory per test, satisfying the single-instance design's caller-precondition isolation.

### State and discovery

Recorded on completion of Phase 1 (2026-07-26). See [the Phase 1 plan](./plans/phase-1-state-and-discovery.md).

**D-108 — Store publication references as `{ location, revision }` keyed by installation id**

The location component of a publication reference is not recoverable from the digest-valued `ModInstallationId` — derivation is one-way. Removing a Discovery Location must cascade its references in a single mutation, so `state` needs the owning location in hand without a second lookup. Storing it is necessary, not redundant: it is the only representation that lets `remove_discovery_location` retain and cascade in one pass over `publication_references`.

**D-109 — Persist the unresolved-quarantine notice in the state document**

`unresolved_quarantine` lives in `AppState`, not only in process memory. A restart after quarantine would otherwise silently forget that publication-reference recovery is unresolved, re-enabling orphan-revision and Asset Store cleanup the design means to keep disabled until the user confirms discard or restores the file.

**D-110 — Only immediate child directories of a Discovery Location are Mod Installations**

The identity model (location id + normalized relative path) can only address content inside the location. A root-level `.mod` descriptor whose `path` field points outside the location is read as advisory display metadata, never as a second installation — `discovery` never follows it to scan elsewhere.

**D-111 — Classify collisions as a pure function over raw names**

Two distinct raw directory entries normalizing to one logical path must be a visible collision, never an arbitrary winner. macOS APFS is case- and normalization-insensitive, so colliding fixtures cannot be created on the development filesystem to test this directly. `classify_entries` is a pure seam over `RawEntry` values precisely so the collision and rejection rules are testable without a real, colliding filesystem.

### Source truth

Recorded on completion of Phase 2 (2026-07-27).

**D-112 — An incomplete source observation is publishable, and it is identity-bearing**

A source with a normalization collision or a rejected entry still establishes a Source Snapshot. Its gaps become Analysis Issues downstream; "evidence absent" is a documented Incomplete Documentation condition, not a fatal one.

But the gaps join the content set in the fingerprint (domain `/v3`), because content alone let a source stop being broken without changing identity: deleting a dangling symlink removes a rejection and touches no enumerated file, so a revision built while the link dangled would verify as unchanged and keep its stale "evidence absent" issue forever.

Only source-determined fields enter. A gap contributes its logical or raw label and a stable `&'static str` reason code per `RejectionReason` discriminant. Three payloads are excluded by name: the escape target (a canonicalized absolute path, so it names the machine's layout), the OS message (host- and locale-dependent), and `io::ErrorKind` (permission state is a property of the machine; `NotFound` and `PermissionDenied` say the same thing to documentation, and folding the kind into durable identity would give one mod two revision identifiers on two machines). The report still carries all three — it is identity that must not.

`Established::Complete | Incomplete` forces the caller to decide what incompleteness means before it can reach the snapshot. `ObservationGaps` remains the single authority on *what* was missed; the enum is only the decision point.

**D-113 — Fixture snapshots are memory-backed, and live verification is a capability**

Only a snapshot established from a live root becomes a `LiveSource`, and only a `LiveSource` can be asked to verify. A fixture snapshot is a bare `SourceSnapshot` and cannot express the question, so there is no "not applicable" arm for a caller to get wrong. The memory backing is compiled only under `test-support`, which makes that a fact about the shipped binary rather than a convention.

`source::fixture::FixtureCorpus` is source-owned rather than a member of `testsupport`, because a fixture must be built by the same construction and fingerprint path a live snapshot is, and the enumeration policy that decides what one may contain is source-owned. `with_file` applies the real policy: a path the policy excludes is an error, not a silently ignored entry. There is deliberately no `from_directory` loader — it would reintroduce exactly the live traversal fixtures exist to avoid.

`with_collision` declares the one gap shape a filesystem here cannot stage (D-111), which is what makes the collision rules testable at all: the gap's effect on the fingerprint, and `read_asset`'s refusal to answer a collided path. It also lets later phases build an *incomplete* fixture observation, which Analysis Issue propagation tests will need.

**D-114 — Home the enumeration-policy version in `source::policy`**

`ENUMERATION_POLICY_VERSION` lives beside the allowlists it versions, and `AnalysisVersionVector::current()` reads it instead of holding a literal. What that makes mechanical is precise and worth not overstating: the version has one home, so `analysis` and `source` cannot drift apart (Meyer's Single Choice). Phase 2A kept a literal in `analysis` that nothing tied to the policy module.

`pinned_policy_surface` remains a **tripwire, not a derivation** — now two-sided. It pins the allowlists and the version side by side so an edit to either fails a test whose comment states the protocol, but it cannot make the bump happen: a developer who edits an allowlist can satisfy it by re-pinning the allowlist alone, and a semantic policy change that touches no constant (`family_for` starting to accept nested `.mod` files, say) does not fail it at all.

This makes `analysis -> source` a real dependency edge, added to the design's permitted-edge list with its cycle check: `source` depends only on `canonical` and `error` and never on `analysis`, so the edge is acyclic.

**D-115 — Memoize one asset observation per logical path, successes and failures alike, and re-verify absences**

`read_asset` freezes the result of the first read for the life of the build. The design already froze successes; failures are the same rule applied to the same question. Two reads that disagreed because the tree moved between them would let one build attach "evidence absent" to a path and then materialize an asset for it.

An asset read for a path already in the snapshot's enumerated content is served from the frozen bytes without touching disk, so one file's bytes have one authority. A path that *collided* is refused as well: enumeration will not pick a winner between two raw entries that normalize alike, and an asset read that fell through to the filesystem would pick one, which is the same silent data loss under another name. Containment applies to asset paths exactly as it does to enumerated links: a resolved target outside the canonical root is `OutsideSourceRoot`, kept distinct from `NotFound` so a containment refusal can never be read as "the mod didn't ship it".

**The freeze is a within-build rule; `verify()` re-observes absences.** A referenced asset the build recorded as absent, which now has readable bytes, is a `Changed` — reported as `SourceChange::appeared`, separate from `assets` because the two invalidate different things: an `AssetChange` invalidates a captured input the manifest quotes, an appearance invalidates an "evidence absent" issue and the placeholder rendered for it. The design's step 7 is "publish only when the current paths and contents still match the candidate's snapshot", and an absent path that now exists no longer matches. Nothing else could catch it — assets are outside the fingerprint by design, and an absence is outside the manifest's referenced-asset set too — so the placeholder would be permanent until some unrelated script edit happened to move the fingerprint. Only absent-to-present counts: one absence becoming another still yields no bytes, and treating it as a change would make publication depend on host permission state — a transient permission blip would abort an otherwise valid build. A collision absence is excluded, because it is already inside the fingerprint through the gap projection, and re-reading it would resolve the NFC spelling onto one of the two colliding entries and report a spurious change on every verify.

That absent-to-absent rule carries a dependency on a module that does not exist yet, recorded so it is not silently inherited: it holds only while Analysis Issue text does not distinguish the absence kinds. `AssetAbsence`'s variants are deliberately not interchangeable — `OutsideSourceRoot` exists precisely so a containment refusal is never read as "the mod didn't ship it" — so if Phase 4 renders the kinds differently, a `NotFound -> Unreadable` transition freezes an issue claiming the mod did not ship a file it demonstrably ships. That is the point to revisit this rule.

**Known limitation — an asset is addressed through its NFC identity.** Enumerated content is read through the raw names the walk observed, but a referenced asset has no observed raw spelling, and the one it had in the script text is destroyed by `LogicalPath::parse`'s NFC normalization one call earlier. So a mod shipping an NFD-named texture — the shape a mod authored on HFS+ and re-zipped produces — reads back as absent on a normalization-sensitive filesystem, which makes the *generated documentation* host-dependent: one machine renders the texture, another renders a placeholder. This is the one place the "never through the normalized identity" rule is not upheld.

Left unfixed rather than fixed blind. macOS cannot stage the failure: APFS (case-sensitive included) and exFAT both normalize in the driver rather than the on-disk format, so writing the NFC spelling *overwrites* an NFD-named file instead of creating a second directory entry — verified on an exFAT disk image. A `read_dir`-and-match fallback on the miss path would therefore ship unexercised. Deferred to the Windows and Linux test harness, alongside the junction and reparse-point question `source::enumerate` records (Phase 12).

### Revision publication

Recorded on completion of Phase 3 tasks 1–2, the minimal revision bundle and atomic publication (2026-07-27). These extend D-090 (revisions owns publication sequencing) and D-093 (the module map and its sanctioned edges).

**D-116 — The Phase 3 manifest carries what Phase 3 can populate**

A bundle manifest carries the bundle schema version, the Mod Installation identifier, the Target Mod and Vanilla Content fingerprints, the analysis version vector, whether the documentation is complete, and the hash of every required entry. Those six are what the Revision identifier is derived over, so the identifier already distinguishes every build the walking skeleton can produce.

The design's remaining manifest content — the required Asset Store keys and the referenced-source-asset input set — is deliberately absent rather than present and empty. Phase 8 is the phase that registers the DDS conversion recipe in `AnalysisVersionVector::asset_recipes`, and that registration changes the digest the identifier nests, hence every Revision identifier, at exactly that moment. Adding the two fields then costs nothing that is not already being paid, while adding them now would model something nothing writes, nothing reads, and no test can meaningfully constrain — and would leave a reader unable to tell "this revision references no assets" from "this build could not have known".

**D-117 — `state` grants publication as a capability handle, not as a store reference**

`revisions` receives a `PublicationCapability`, which delegates one compare-and-swap on one Mod Installation's publication reference and exposes nothing else. `StateStore`'s own compare-and-swap is visible only inside `state`, so the handle is not merely the polite route but the only expressible one from any other module.

That is what makes D-090's "`revisions` cannot modify unrelated settings or access the mutable state representation" structural rather than a matter of discipline. A `&StateStore` would have carried Discovery Location configuration, quarantine acknowledgement, and the whole snapshot along with it. The capability borrows rather than owns: the store's lifetime is the process and the composition root holds it, so granting is an explicit act at a place where a `&StateStore` is legitimately in hand.

The caller still owes one guarantee neither layer can check — that the installation and the location handed in are a coherent pair, since `ModInstallationId` derivation is one-way. That obligation is stated once, on the capability method, because passing the capability down does not move it.

**D-118 — Bundle validation is the reader-shaped observation this phase ships**

One function decides whether a directory is a Documentation Revision bundle: it reads the manifest back from disk, re-derives the identifier from the manifest's own body, re-hashes every required entry, and reports everything the directory holds that the manifest does not account for. It proves integrity, not readability — it hashes bytes without parsing them — and it reports the whole disagreement rather than the first finding, because a person diagnosing a damaged bundle needs all of it and the protocol's adopt-or-refuse decision rests on which kinds are present.

**Unaccounted-for material is reported and does not decide validity.** The design's list is "the manifest, every required entry, and a content-valid Asset Store proof" (docs/technical-design.md, "Revision bundles"); "and nothing else is present" is this protocol's addition, and it answers a different question. Every other finding names something the revision cannot serve. This one names material *beside* a revision that is entirely intact — and since a published path is never deleted (D-119) and the Revision identifier is a pure function of content, folding it into validity would let one `.DS_Store` that Finder wrote into a published directory make that revision permanently unreadable *and* unrebuildable: every rebuild derives the same identifier, lands on the same occupied path, and is refused. There is no repair path in this phase. So the finding is carried on the validation proof and decides the one thing it is genuinely load-bearing for, in D-119.

The publication protocol, STE-17's Revision Reader, and Phase 9's Validate Published Revision are the same question asked of a directory. STE-17 wraps this path rather than replacing it; a second implementation would be a second authority on what a bundle is, and the two would diverge in the direction that matters — one accepting what the other rejects.

A separate entry point answers the second question, "and is it *this* revision's". That a directory under `bundles/` is named by its own manifest's identifier is a property of this protocol's writes, not of the directory, so it is checked rather than assumed by every caller that arrived by identifier.

**D-119 — An occupied final path is adopted or refused, never replaced**

A publication whose final path already holds a bundle adopts it when it validates, names the expected revision, and holds nothing the manifest does not account for; it refuses otherwise. A published path is never deleted: a pinned reader may hold it, and whether the occupant is a damaged copy of this revision or an intact copy of another one, repair is a retention concern for a later phase.

The third condition is where D-118's reported-but-not-disqualifying finding decides something, and it is the only place it does. Commit point 1 is the moment this protocol claims a directory, entirely, *is* revision X — the claim a later reader, retention pass, and asset-key enumeration all inherit — so inheriting material it cannot account for is what it must refuse. The same rule applies at both routes into that commit point: the staged tree about to be moved, so a bundle carrying a stowaway is never created either, and the occupant about to be adopted. Reading such a directory stays permitted, so the user-visible consequence of a stray file is a diagnosable refusal that names the file, rather than a revision that has silently become unopenable.

On refusal the staging directory is retained, as the one directory a person diagnosing this can compare against an occupant nothing is allowed to delete. It is a diagnostic and not recoverable state — nothing reads a staging directory back — so it is swept at the next open like any other abandoned attempt. **The retention sweep's rule over `staging/` stays unconditional**, which is the half of this that STE-17 must write against: a sweep that had to ask whether an attempt was retained on purpose would need a record of intent outliving the process that formed it, kept to protect material no code path consumes.

Adoption is not one of several routes; it is the only one. `fs::rename` refuses a non-empty destination *directory* — POSIX returns `ENOTEMPTY` — unlike the file replacement `state::replace` depends on, where rename overwrites the destination by design. The two modules therefore cannot share a mental model of "move onto the final path", and the difference is empirical rather than stylistic. This is also the expected shape after a crash between the two commit points: an identical rebuild derives the identifier the interrupted attempt derived, finds its own completed work, and finishes the publication instead of failing forever.

**D-120 — `revisions` owns the Revision Candidate type**

The Revision Candidate — the installation it documents, the source fingerprints it was built from, the analysis versions that produced it, its completeness, and its documents — is defined in `revisions`, the module that consumes it and derives identity from it. It carries no Discovery Location, because a location's path is editable configuration and a rebind must not invalidate a revision, and no document carries a caller-supplied path, because a path would leak bundle layout and from Phase 6 would carry raw Stellaris identifiers into the filesystem.

**This creates no `analysis -> revisions` edge.** An earlier draft of this entry claimed Phase 6 would add one and excused it as "type-only and acyclic", which is both the excuse the engineering principles name as a diagnostic and a direct contradiction of D-122's cycle check. It is also unnecessary: the candidate is assembled by the application layer, which sits above every deep module and already holds both halves — "application use cases supply the validated Revision Candidate and do not separately move the bundle or mutate state" (docs/technical-design.md, "Revision bundles"). `analysis` produces its own output types and never names a candidate; the use case maps that output into one. So the edge runs `application -> revisions` and `application -> analysis`, both downward, and the only peer edge in play is `revisions -> analysis` for the version-vector value type, which D-122 records.

The alternative — homing the candidate in `analysis` — would make the producer the authority over what publication accepts, and would put the parse that refuses two documents of one identity somewhere other than the module whose invariant it protects.

**D-121 — An unconfirmed post-move directory flush refuses the pointer commit**

After the bundle reaches its final path, the `bundles/` directory entry naming it is flushed. If that flush fails, publication stops: nothing is published and the pointer is unchanged.

This is deliberately asymmetric with `state`, which floors an equivalent uncertainty at `CommittedDurabilityUncertain` and advances. There the new state *is* what a reader sees, and reporting otherwise would be a lie about visible fact. Here the pointer commit would be a *further irreversible action taken on an unconfirmed foundation*: a pointer naming a bundle that a crash can still erase makes every read fail closed, and it breaks retention's assumption that a published pointer names a complete bundle. Refusing costs one rebuild, which then takes the adoption path of D-119 and flushes again.

**"Refusing costs one rebuild" is only true where the flush is an operation that can succeed**, and on Windows it is not one — the premise was false on a first-release target when this was first written, which would have made the cost every rebuild, forever. That qualifier is not repaired here but in D-123, which is the correct home for it: an error reaching this decision always means an attempted flush was refused, never that the platform had nothing to offer. Both modules' asymmetry is then about what each knows after a *real* flush failure, not about which platform they run on.

**D-122 — Three identity-type edges from `revisions` — proposed additions to the sanctioned edge list**

`revisions` imports `ModInstallationId` and `DiscoveryLocationId` from `discovery`, `SourceFingerprint` and `ContentHash` from `source`, `AnalysisVersionVector` from `analysis`, and `LogicalPath` and the encoding and hex primitives from `canonical`. The sanctioned direct deep-module edge list (docs/technical-design.md, "Rust package and dependency direction") names `revisions -> state` alone. This entry exists so the gap is decided rather than inherited. **Amending that list is the maintainer's act, not this branch's**, so what follows is written as the proposal to apply, in the form the list uses.

`revisions -> canonical` is **not part of the proposal and needs no entry**. `canonical` is explicitly a leaf primitive below the deep-module row (D-105), the way `error` is; the list governs peer edges among the deep modules, and depending on a leaf is not one. D-123's `durability` is a leaf on the same footing and is likewise not an edge.

The three that do belong on the list, each with the owner and cycle check the list requires:

- **`revisions -> discovery`** for the Mod Installation and Discovery Location identifiers a manifest and a publication reference are keyed by. Owner: `discovery`, which mints both and owns their derivation and rendered form. Cycle check: `discovery` depends on `canonical` and `error` and never on `revisions`; nothing discovery does needs a bundle, and a bundle is produced only after discovery has finished. Design mandate: "Every revision manifest records its Mod Installation identifier" (docs/technical-design.md, "Installation identity").
- **`revisions -> source`** for `SourceFingerprint` and `ContentHash`, the exact input fingerprints the manifest records and the identifier is derived over. Owner: `source`, which owns the fingerprint construction and the enumeration policy whose version it quotes. Cycle check: `source` depends on `canonical` and `error` only; it is already the target of `analysis -> source` for the same class of reason (D-114) and has no path back. Design mandate: "exact input fingerprints" (docs/technical-design.md, "Revision bundles").
- **`revisions -> analysis`** for `AnalysisVersionVector`, the complete analysis version vector the manifest records and the identifier nests as one digest. Owner: `analysis`, which owns the vector's components and the schedule on which they change. Cycle check: acyclic, and this is the part the first draft of this entry got wrong. It claimed the check held while D-120 asserted that Phase 6 would add `analysis -> revisions`; both could not be true, and "it is type-only" is precisely the excuse the engineering principles flag rather than a resolution. **D-120's claim is withdrawn**, which is what makes this check sound rather than merely asserted: the Revision Candidate is assembled by the application layer, which sits above every deep module and already holds analysis's output — "application use cases supply the validated Revision Candidate and do not separately move the bundle or mutate state" (docs/technical-design.md, "Revision bundles"). `analysis` never names a candidate, a bundle, or a manifest, so it acquires no dependency on `revisions` in Phase 6 or later. Design mandate: "complete analysis version vector" (docs/technical-design.md, "Revision bundles").

The argument for the three: every one is a stored identity value the manifest holds and the identifier is derived over, not a behavioural dependency. `revisions` calls no traversal, no hashing of a source tree, and no analysis; it holds values those modules minted and renders them. The nearest precedent is `analysis -> source`, which is on the list precisely because D-114 put it there when the version vector started quoting a source-owned constant — a value-type edge, named with its owner and its cycle check rather than left implicit. `state -> discovery` exists in the code for the identical reason (publication references are keyed by `ModInstallationId`) and is likewise unlisted, but it is the weaker precedent: `state` sits in the row above the deep modules, so its edge is downward rather than peer-to-peer.

The argument against: the edge list is the mechanism by which "additional edges require explicit ownership and a cycle check" is enforced, and "it is only a value type" is exactly the excuse that erodes such a list one import at a time. A design document that does not name an edge the code has does not describe the code, and that discrepancy stands until this proposal is accepted or refused.

The fallback if they are refused: `revisions` defines its own fixed-width hex newtypes for each identity and converts at the application boundary, the way `state::RevisionId` already stays an opaque round-trip token. The cost is a second authority over each identity's rendered form — the manifest's spelling of a Mod Installation identifier would no longer be the same code as `discovery`'s — and a conversion layer whose only job is to restate what the two types agree on. That cost is why the edges are proposed rather than pre-emptively avoided.

**D-123 — One home for "can this platform flush a directory", and what its absence costs**

`state::replace` and `revisions::publish` each reach a commit point whose durability rests on a directory's entries reaching disk, and both spelled that as `File::open(dir)?.sync_all()`. On Windows that call cannot succeed: `CreateFileW` refuses a directory without `FILE_FLAG_BACKUP_SEMANTICS`, which `File::open` does not set, and `FlushFileBuffers` on a directory handle is not a supported operation even given the handle. `state` therefore degraded on Windows — every replacement floored at `CommittedDurabilityUncertain`, so no superseded bundle would ever have become retirable — while `revisions` stopped dead: D-121 makes the refusal fatal, so no revision could be published on that platform at all.

The decision is homed once, in a `durability` leaf module below the deep-module row, rather than fixed twice. It opens the directory with the flag Windows requires and reads a refusal of the flush itself as "this platform does not provide the operation", reporting success. The open stays outside that tolerance on every platform, so a directory that was never created is still an error at a commit point. The tolerance is written over the platform and not over the error code, which matters: raw code 5 is `ERROR_ACCESS_DENIED` on Windows and `EIO` on POSIX, and a tolerance keyed on the code would silently swallow a device failure on the platform this project is developed on. A negative control asserts exactly that.

**This is a durability weakening on one platform and is recorded as one.** NTFS is why it is defensible: it journals metadata, so creating, renaming, and removing a directory entry are logged transactions ordered ahead of the changes they describe, and a crash replays the journal rather than producing the POSIX shape this flush exists to prevent. The residual risk is real. On NTFS the application cannot force the journal to the platter at a chosen moment the way `fsync` can, so a crash inside its write-back window can still lose a recent entry. On exFAT, FAT32, and SMB shares — a removable drive, a redirected application-data directory — there is no journal standing in for anything. Both callers' worst case is an absent state document or an absent bundle rather than a damaged one, both fail closed on absence, and the Revision identifier is a pure function of content, so the same rebuild republishes to the same path and repairs it. What is given up is the guarantee, not the recovery.

Its correctness on Windows rests on the Win32 contract cited above and nothing else — it cannot be executed on the development machine. The Phase 12 Windows packaged smoke test is where it becomes an observation, alongside the rename-semantics claims `state::replace` and `revisions` already carry there (docs/technical-design.md, "Verification architecture").

## Provisional decisions

**P-004 — Adopt the extracted serializable Result package**

Expected application outcomes use the plain `{ ok: true, value } | { ok: false, error }` envelope from the user's Result package across both Tauri and HTTP. Rust keeps native typed results internally and transport adapters map them into that wire contract.

Authorization, malformed transport input, programmer errors, framework control flow, and unexpected failures remain transport-level failures rather than ordinary Result errors. Host adapters represent unexpected failures with a typed internal error before emitting an opaque transport failure with a correlation identifier; a Rust panic is never part of the operation contract. Payloads must be JSON-safe; void success cannot rely on JavaScript `undefined`.

Until publication, the application vendors the equivalent two-variant type and minimal utilities locally behind the intended import boundary, so MVP work does not block on extraction. Adoption requires publishing under an MIT-compatible license and stable name and pinning it through the lockfile.

The cross-language suite covers every operation, every success shape, every expected-error union member, and malformed-envelope negative controls through both transports. Replacing the local module with the package must not change those tests.

## Deferred features

- Full Playset composition and conflict resolution.
- Save parsing and personalized next-step guidance.
- Documentation Export, including wiki-style Markdown.
- AI integrations over generated knowledge.
- Deep Player Documentation for object types beyond the technology vertical slice.
- Full Event Chain documentation after the technology slice proves the shared model.
- Automatic classification of player, AI-only, developer, demo, and console-only routes.
- Derived probability, progress-bound, and outcome analysis for Event Chains.
- Semantic grouping of equivalent Grant Sites and reconverging route branches.
- Search indexing for content categories beyond technologies, megastructures, buildings, and ship components.
- Interactive Concept Links that open linked documentation in a popover.
- Reliable detection and display of Paradox launcher enabled/disabled state.
- Continuous source watching and live documentation regeneration for mod authors.

## Validated feasibility notes

### DDS technology icons

The original note recorded 976 vanilla and 1,550 modded technology DDS icons in uncompressed RGB/RGBA plus DXT1, DXT3, and DXT5 variants, and reported that one vanilla and one modded DXT5 icon converted to 52×52 RGBA PNG and looked right.

The completed [DDS evaluation](./spikes/dds-evaluation.md) supersedes it and corrects its emphasis. Across 33,145 measured files, DXT5 is 114 of the 2,621 files on a technology path — the two icons that were converted were the rarest class of the set they stood for, while 1,257 uncompressed 32-bit and 880 uncompressed 24-bit icons had never been decoded. The corpus also contains classes the note did not list: a 16-bit A1R5G5B5 layout, a DX10 header, 844 block-unaligned compressed surfaces, 520 24-bit surfaces with unaligned rows, 12 cube maps, and two files that are not DDS at all.

`image_dds` is accepted at `0.7.2` with encoding disabled ([ADR 0008](./adr/0008-decode-source-textures-through-a-pinned-conversion-recipe.md)). Correctness is established by an independent second reading rather than by inspection: the failure mode here is a plausible image with its channels exchanged, and only eleven files in the corpus can expose it.

Resolving a game concept to the correct texture remains a separate and larger question than conversion, and belongs to `analysis`.

### Playset composition

Stellaris composition is not uniformly file-level or last-wins. It can include exact-path shadowing, directory replacement, named-definition collisions, and content-type-specific first-wins, last-wins, or duplicate behavior. This is the reason Playset support is deferred behind a dedicated resolver.

### Local browser serving

Tauri can package the desktop application and serve web assets, but it does not provide a complete LAN companion product automatically. The embedded service must supply the documentation API, LAN binding, access control, and companion discovery or pairing.

### Parsing cost

Parsing the pinned corpora through the spike adapter takes a median 211 ms serially and 63 ms in parallel for the 53.5 MiB vanilla script set, and 133 ms and 39 ms for Gigastructural Engineering, at a peak resident set of 194 MiB with every corpus held at once. The wrapper costs 10 to 35% over Jomini's tape.

Parsing is therefore roughly an order of magnitude cheaper than the second-pass hashing measured beside it, so the deferred choice between an awaited command and a host-owned job should not be decided on parsing cost. These are spike-harness measurements rather than the real adapter in a real build, and carry the same directional caveat as the fingerprint figures below. See [Parser evaluation](./spikes/parser-evaluation.md).

### Fingerprint verification cost

A preliminary local measurement hashed a broad set of script, localization, and UI-definition files from the installed macOS corpus:

| Corpus | Files | Bytes | Elapsed |
| --- | ---: | ---: | ---: |
| Vanilla | 6,864 | 213,586,670 | 1.78s |
| ACOT | 1,152 | 19,938,493 | 0.23s |
| Gigastructures | 2,161 | 66,000,693 | 0.52s |
| Acquisition of Technology | 652 | 8,058,724 | 0.13s |

This is a directional filesystem-and-SHA-256 measurement rather than the final Rust implementation benchmark. It excludes large unrelated binary assets and includes a deliberately broad Vanilla file set. It suggests that a second Target Mod verification pass is subsecond on the current machine, while a complete Vanilla pass is noticeable but modest and should benefit from the separately validated Vanilla cache.

## Open decisions

**Q-002 — Project identity**

The project name and copyright-holder wording are not yet chosen. Create the root `LICENSE` file after those are known.

**Q-004 — Remaining Resolution Profile cells**

Pre-implementation oracle investigation is complete, but the Resolution Profile remains partial. Implement the resolved rows first and use resolver conformance traces plus focused oracle fixtures to close the remaining cells. An unresolved cell blocks support for the content type that requires it, not implementation of the resolver module or unrelated resolved content types.

See [Resolver evaluation](./spikes/resolver-evaluation.md).
