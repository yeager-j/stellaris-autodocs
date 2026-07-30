# Decision Log

Last updated: 2026-07-30

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

**D-129 — The metadata accelerator is deferred to Phase 9, and a hint may only accelerate the freshness pre-check**

Recorded 2026-07-28 for a decision taken 2026-07-27 (STE-13), so that Phase 2's one unimplemented task is a decision on the record rather than an omission a later reader has to reconstruct. D-085 and the design's `source` responsibilities are unchanged: the module still owns disposable metadata accelerators. This is scheduling plus a correctness boundary, not a change of ownership.

**Deferred because the optimization's denominator does not exist yet.** Measured after Phase 2B landed rather than guessed: across Vanilla 4.4.6 and three large mods, `stat` is 3–86 ms while read-and-hash is 87 ms–1.06 s, so hashing is 86–97% of a freshness check and a vanilla-plus-large-mod check would fall from roughly 0.85 s to about 0.05 s. Warm-page-cache figures *understate* it, because cold reads cost more while `stat` stays flat — the design's slow-storage case is where the win is largest. (Those are Python `hashlib` timings, roughly 2× the Rust implementation, which measured vanilla verification at 0.52 s; the ratio is what is stable across corpora, and the ratio is the claim.) What none of that answers is whether ~0.85 s matters, because parser, resolver, generation, and asset conversion do not exist, so there is no build total to weigh it against. Phase 3 was the critical path, the deferral is cheap to reverse, and `source::scan` — what an Ensure freshness pre-check calls — is already the integration point.

**Where a hint is permitted is a correctness boundary, not a tuning knob**, and it is recorded now because it constrains the eventual implementation rather than following from it. A stat hint substitutes assumed identity for proven identity. In the Ensure freshness pre-check — the "Checking for changes…" phase — a false "unchanged" shows stale documentation until the user rebuilds: recoverable. In pre-publication verification (the design's "Source snapshot consistency" step 6) the same false answer publishes a revision whose manifest claims inputs that were never verified, which is durable corruption of revision identity and makes "authoritative" stop meaning authoritative. So hints accelerate the pre-check only, and the exclusion costs nothing: a real build reads the bytes anyway, so step 6 has no hint to gain from. The boundary should be expressed in the types if practical, so the accelerated path is not reachable from verification.

**STE-15 rides with it.** Bounding the memory a Source Snapshot may consume is a subissue that auto-closes with STE-13, and the two are one conversation — a private temporary-file backing bounds resident memory without bounding the single-file allocation, so the storage-backing and size-cap decisions constrain each other. It is semi-trusted-input handling with a Phase 12 release-gate deadline, so if STE-13 is ever closed unimplemented it must be detached first and kept open.

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

The publication protocol, the Revision Reader, and Phase 9's Validate Published Revision are the same question asked of a directory. The reader wraps this path rather than replacing it (D-124); a second implementation would be a second authority on what a bundle is, and the two would diverge in the direction that matters — one accepting what the other rejects.

A separate entry point answers the second question, "and is it *this* revision's". That a directory under `bundles/` is named by its own manifest's identifier is a property of this protocol's writes, not of the directory, so it is checked rather than assumed by every caller that arrived by identifier.

**D-119 — An occupied final path is adopted or refused, never replaced**

A publication whose final path already holds a bundle adopts it when it validates, names the expected revision, and holds nothing the manifest does not account for; it refuses otherwise. A published path is never deleted: a pinned reader may hold it, and whether the occupant is a damaged copy of this revision or an intact copy of another one, repair is a retention concern for a later phase.

The third condition is where D-118's reported-but-not-disqualifying finding decides something, and it is the only place it does. Commit point 1 is the moment this protocol claims a directory, entirely, *is* revision X — the claim a later reader, retention pass, and asset-key enumeration all inherit — so inheriting material it cannot account for is what it must refuse. The same rule applies at both routes into that commit point: the staged tree about to be moved, so a bundle carrying a stowaway is never created either, and the occupant about to be adopted. Reading such a directory stays permitted, so the user-visible consequence of a stray file is a diagnosable refusal that names the file, rather than a revision that has silently become unopenable.

On refusal *at the final path* the staging directory is retained, as the one directory a person diagnosing this can compare against an occupant nothing is allowed to delete. **The staged route makes the opposite choice for the same finding, and the asymmetry is deliberate**: a staged tree is this process's own scratch work, written moments ago from a candidate the caller still holds and reproducible by repeating the build, and the refusal already names the offending entries — so keeping it preserves nothing the refusal does not carry. What earns retention at the final path is that the occupant is neither ours to delete nor ours to reproduce. It is a diagnostic and not recoverable state — nothing reads a staging directory back — so it will be swept at the next open like any other abandoned attempt. *Will*: this phase writes no sweep, so retained attempts currently survive across launches and accumulate one per refusal, which is inert but is not the bound the rule describes. **The retention sweep's rule over `staging/` stays unconditional**, which is the half of this that Phase 9's sweep must write against: a sweep that had to ask whether an attempt was retained on purpose would need a record of intent outliving the process that formed it, kept to protect material no code path consumes. The Revision Reader landed without that sweep deliberately (D-126) — its precondition names facts `revisions` cannot observe — so the accumulation this paragraph describes is still unbounded across launches.

Adoption is not one of several routes; it is the only one. `fs::rename` refuses a non-empty destination *directory* — POSIX returns `ENOTEMPTY` — unlike the file replacement `state::replace` depends on, where rename overwrites the destination by design. The two modules therefore cannot share a mental model of "move onto the final path", and the difference is empirical rather than stylistic. This is also the expected shape after a crash between the two commit points: an identical rebuild derives the identifier the interrupted attempt derived, finds its own completed work, and finishes the publication instead of failing forever.

**D-120 — `revisions` owns the Revision Candidate type**

The Revision Candidate — the installation it documents, the source fingerprints it was built from, the analysis versions that produced it, its completeness, and its documents — is defined in `revisions`, the module that consumes it and derives identity from it. It carries no Discovery Location, because a location's path is editable configuration and a rebind must not invalidate a revision, and no document carries a caller-supplied path, because a path would leak bundle layout and from Phase 6 would carry raw Stellaris identifiers into the filesystem.

**This creates no `analysis -> revisions` edge.** An earlier draft of this entry claimed Phase 6 would add one and excused it as "type-only and acyclic", which is both the excuse the engineering principles name as a diagnostic and a direct contradiction of D-122's cycle check. It is also unnecessary: the candidate is assembled by the application layer, which sits above every deep module and already holds both halves — "application use cases supply the validated Revision Candidate and do not separately move the bundle or mutate state" (docs/technical-design.md, "Revision bundles"). `analysis` produces its own output types and never names a candidate; the use case maps that output into one. So the edge runs `application -> revisions` and `application -> analysis`, both downward, and the only peer edge in play is `revisions -> analysis` for the version-vector value type, which D-122 records.

The alternative — homing the candidate in `analysis` — would make the producer the authority over what publication accepts, and would put the parse that refuses two documents of one identity somewhere other than the module whose invariant it protects.

**D-121 — An unconfirmed post-move directory flush refuses the pointer commit**

After the bundle reaches its final path, the `bundles/` directory entry naming it is flushed. If that flush fails, publication stops: nothing is published and the pointer is unchanged.

This is deliberately asymmetric with `state`, which floors an equivalent uncertainty at `CommittedDurabilityUncertain` and advances. There the new state *is* what a reader sees, and reporting otherwise would be a lie about visible fact. Here the pointer commit would be a *further irreversible action taken on an unconfirmed foundation*: a pointer naming a bundle that a crash can still erase makes every read fail closed, and it breaks retention's assumption that a published pointer names a complete bundle. Refusing costs one rebuild, which then takes the adoption path of D-119 and flushes again.

**"Refusing costs one rebuild" is only true where the flush is an operation that can succeed**, and when this was first written it was not one on Windows — the premise was false on a first-release target, which would have made the cost every rebuild, forever. That qualifier is not repaired here but in D-123, which is the correct home for it: an error reaching this decision always means a flush was attempted and refused, never that the platform had nothing to offer. Both modules' asymmetry is then about what each knows after a *real* flush failure, not about which platform they run on.

D-123 has since narrowed what "nothing to offer" covers — the flush is expected to succeed on a local Windows volume once the handle carries the access right `FlushFileBuffers` requires, and only redirectors and non-journalling filesystems are excused — but this decision does not depend on how wide that set is. It depends only on the guarantee that the set is not everything, so that refusing here can cost a rebuild rather than every rebuild.

**The boundary, stated once for both decisions.** This refusal is scoped to *a flush that was attempted and failed*. On a volume that provides no directory flush at all, publication **proceeds** and reports `BundleDurability::NotProvidedByPlatform` in its success outcome, so a caller can report the weakening; it does not refuse. The line between the two is not a matter of degree. An attempted-and-failed flush is ambiguous evidence about one directory at one moment, and one rebuild resolves it. A flush the volume never provides is a permanent, knowable property of that volume, so the identical refusal would mean a mod library on a network share or a removable drive never publishes at all — the failure mode D-123 was reopened to eliminate, reappearing one level down. The distinction is carried in the type `durability::sync_dir` returns rather than in prose, and cannot be flattened by accident: `DirectoryFlush` is `#[must_use]`, so discarding it is an error under `-D warnings`.

What proceeding costs on such a volume, said plainly: a crash or power loss in the seconds around a publication can leave the newly written bundle's directory entry, or the state document naming it, missing at the next launch. Nothing already published is damaged, reads fail closed on absence, and because the Revision identifier is a pure function of content the same rebuild republishes to the same path and repairs it. `state` reaches the same platform fact independently and floors at `CommittedDurabilityUncertain` (D-123), so the two halves of one publication agree on such a volume without either consulting the other.

**D-122 — Three identity-type edges from `revisions`, accepted and added to the sanctioned edge list**

`revisions` imports `ModInstallationId` and `DiscoveryLocationId` from `discovery`, `SourceFingerprint` and `ContentHash` from `source`, `AnalysisVersionVector` from `analysis`, and `LogicalPath` and the encoding and hex primitives from `canonical`. The sanctioned direct deep-module edge list (docs/technical-design.md, "Rust package and dependency direction") named `revisions -> state` alone, so the document described a different program than `src-tauri`.

**Accepted, and the amendment is applied in this branch.** The three edges below are now on that list, each carrying the owner and cycle check the list requires. The same section also gains a sentence naming the three leaf primitives that sit below the deep-module row — `canonical`, `error`, and D-123's `durability` — and stating that the map excludes them by convention rather than by oversight, so a reader learns they exist without the map pretending to be exhaustive. This is deliberately not deferred: the design document is authority for this project, and a contract-tier ticket is the moment an edge list is decided rather than the moment a discrepancy is inherited.

`revisions -> canonical` is **not part of this and needs no entry**. `canonical` is explicitly a leaf primitive below the deep-module row (D-105), the way `error` is; the list governs peer edges among the deep modules, and depending on a leaf is not one. D-123's `durability` is a leaf on the same footing and is likewise not an edge — it is named in the prose beside the module map, not in the map or the edge list, which is where `canonical` and `error` have always been described.

The three that do belong on the list, each with the owner and cycle check the list requires:

- **`revisions -> discovery`** for the Mod Installation and Discovery Location identifiers a manifest and a publication reference are keyed by. Owner: `discovery`, which mints both and owns their derivation and rendered form. Cycle check: `discovery` depends on `canonical` and `error` and never on `revisions`; nothing discovery does needs a bundle, and a bundle is produced only after discovery has finished. Design mandate: "Every revision manifest records its Mod Installation identifier" (docs/technical-design.md, "Installation identity").
- **`revisions -> source`** for `SourceFingerprint` and `ContentHash`, the exact input fingerprints the manifest records and the identifier is derived over. Owner: `source`, which owns the fingerprint construction and the enumeration policy whose version it quotes. Cycle check: `source` depends on `canonical` and `error` only; it is already the target of `analysis -> source` for the same class of reason (D-114) and has no path back. Design mandate: "exact input fingerprints" (docs/technical-design.md, "Revision bundles").
- **`revisions -> analysis`** for `AnalysisVersionVector`, the complete analysis version vector the manifest records and the identifier nests as one digest. Owner: `analysis`, which owns the vector's components and the schedule on which they change. Cycle check: acyclic, and this is the part the first draft of this entry got wrong. It claimed the check held while D-120 asserted that Phase 6 would add `analysis -> revisions`; both could not be true, and "it is type-only" is precisely the excuse the engineering principles flag rather than a resolution. **D-120's claim is withdrawn**, which is what makes this check sound rather than merely asserted: the Revision Candidate is assembled by the application layer, which sits above every deep module and already holds analysis's output — "application use cases supply the validated Revision Candidate and do not separately move the bundle or mutate state" (docs/technical-design.md, "Revision bundles"). `analysis` never names a candidate, a bundle, or a manifest, so it acquires no dependency on `revisions` in Phase 6 or later. Design mandate: "complete analysis version vector" (docs/technical-design.md, "Revision bundles").

The argument for the three: every one is a stored identity value the manifest holds and the identifier is derived over, not a behavioural dependency. `revisions` calls no traversal, no hashing of a source tree, and no analysis; it holds values those modules minted and renders them. The nearest precedent is `analysis -> source`, which is on the list precisely because D-114 put it there when the version vector started quoting a source-owned constant — a value-type edge, named with its owner and its cycle check rather than left implicit. `state -> discovery` exists in the code for the identical reason (publication references are keyed by `ModInstallationId`) and is likewise unlisted, but it is the weaker precedent: `state` sits in the row above the deep modules, so its edge is downward rather than peer-to-peer.

The argument against, which acceptance answers rather than dismisses: the edge list is the mechanism by which "additional edges require explicit ownership and a cycle check" is enforced, and "it is only a value type" is exactly the excuse that erodes such a list one import at a time. That is why each of the three is written onto the list with a named owner and a stated cycle check instead of being left implicit — the discipline the list exists to impose is the discipline that was applied to admit them.

The alternative, rejected: `revisions` defines its own fixed-width hex newtypes for each identity and converts at the application boundary, the way `state::RevisionId` stays an opaque round-trip token. The cost is a second authority over each identity's rendered form — the manifest's spelling of a Mod Installation identifier would no longer be the same code as `discovery`'s — and a conversion layer whose only job is to restate what the two types agree on. `state::RevisionId` is not a counter-example: it round-trips a token it never interprets, whereas a manifest renders these identities and is compared byte-for-byte against what the owning module renders.

**D-123 — One home for "can this platform flush a directory", and what its absence costs**

`state::replace` and `revisions::publish` each reach a commit point whose durability rests on a directory's entries reaching disk, and both spelled that as `File::open(dir)?.sync_all()`. On Windows that call cannot succeed, because `CreateFileW` refuses a directory without `FILE_FLAG_BACKUP_SEMANTICS` and `File::open` does not set it: *"To open a directory using **CreateFile**, specify the **FILE_FLAG_BACKUP_SEMANTICS** flag as part of dwFlagsAndAttributes"* (<https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew>). `state` therefore degraded on Windows — every replacement floored at `CommittedDurabilityUncertain`, so no superseded bundle would ever have become retirable — while `revisions` stopped dead: D-121 makes the refusal fatal, so no revision could be published on that platform at all.

The decision is homed once, in a `durability` leaf module below the deep-module row, rather than fixed twice. It is a third module on exactly D-105's footing, and is treated the way D-105's two already are: named in the prose beside the design's module map rather than in the map, and appearing in no edge (D-122). The map names the deep-module row and its adapters; a primitive below that row is not one. The open stays outside the tolerance on every platform, so a directory that was never created is still an error at a commit point.

**The access right, and the mistake this entry now records.** The first version of this decision opened the directory `.read(true)` only, and read the resulting `ERROR_ACCESS_DENIED` as evidence that "`FlushFileBuffers` on a directory handle is not a supported operation". That was a misreading of the contract, and it is corrected here rather than quietly rewritten, because it is the reason the rest of this entry is written the way it is. `FlushFileBuffers` documents exactly one access requirement — *"The file handle must have the **GENERIC_WRITE** access right"* (<https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers>) — and says nothing about directory handles being unsupported. The refusal was the code's own access mode, not the platform's answer.

Two things followed from that, and both are why this was worth reopening. First, the tolerance was **unfalsifiable**: with every flush excused, `sync_dir` returned `Ok` for any openable path on Windows, so neither `state`'s `CommittedDurabilityUncertain` nor `revisions`'s `BundleDurabilityUnconfirmed` was reachable there — precisely the distinction D-121 and `revisions::publish` assert the code can make. Second, the weakening may have been **self-inflicted**: the field report the tolerated codes come from (<https://github.com/hypermodeinc/badger/issues/699>) opened directories with `GENERIC_READ | GENERIC_WRITE | FILE_FLAG_BACKUP_SEMANTICS` and saw `FlushFileBuffers` succeed on a local drive, failing with `ERROR_INVALID_FUNCTION` only over an SMB redirector.

So the module now asks for the right the contract names: on Windows it opens with `FILE_FLAG_BACKUP_SEMANTICS` and write access, falling back to a read-only handle only if that open is refused, and it carries which handle it got into the classification. On a write-capable handle the tolerated set is `ERROR_INVALID_FUNCTION` and `ERROR_NOT_SUPPORTED` — the "I do not implement this" answers, attested for exactly this call. `ERROR_ACCESS_DENIED` is tolerated only on the read-only fallback, where the documented `GENERIC_WRITE` requirement makes it a statement about the handle rather than about the directory; on a write-capable handle it is a real denial and is reported.

The tolerance is written over the platform and not over the error code, which still matters: raw code 5 is `ERROR_ACCESS_DENIED` on Windows and `EIO` on POSIX, and a tolerance keyed on the code alone would silently swallow a device failure on the platform this project is developed on. The platform gate and the Windows table are separate functions, and the table is compiled on every platform on purpose — the defect above lived in a `#[cfg(windows)]` branch that no local gate compiled, so the citation was the only evidence there was. Separating them makes the table falsifiable where the project is developed; negative controls on both fire.

**Verification, and what it is now rather than what it was.** The Windows arm is compiled and executed by a `windows-latest` leg of the CI matrix (`.github/workflows/ci.yml`), added for this reason: without it the next edit to this file is unchecked by anything, and the previous round's citation error would have been invisible again. That leg runs a test asserting that an ordinary local directory yields a write-capable handle whose flush is **not** refused — which is the empirical claim this entry rests on, and a red there is the report that it is wrong.

**Where the tolerance fires it is still a durability weakening, and is recorded as one.** It is now bounded to redirectors and filesystems that answer "incorrect function" rather than to Windows as such. On exFAT, FAT32, and SMB shares — a removable drive, a redirected application-data directory — there is no metadata journal standing in for the flush and the redirector may refuse outright. Both callers' worst case is an absent state document or an absent bundle rather than a damaged one, both fail closed on absence, and the Revision identifier is a pure function of content, so the same rebuild republishes to the same path and repairs it. What is given up is the guarantee, not the recovery.

**The outcome is three-way, because "not performed" is not "performed".** The first correction above still converted a tolerated refusal into `Ok(())`, indistinguishable from a flush that had happened — so `state` returned `Committed` and `revisions` committed its pointer over a directory entry a crash could still erase, both reporting a durability nothing had observed. `sync_dir` now returns `io::Result<DirectoryFlush>`: `Ok(Flushed)` for a performed flush, `Ok(NotProvided)` for one this platform and filesystem do not offer, and `Err` for one attempted and refused. `DirectoryFlush` is `#[must_use]`, so a caller cannot discard the answer silently; and the three-way decision is a small function separated from the platform gate, compiled and exercised on every platform for the same reason the tolerance table is — off Windows the `NotProvided` arm is unreachable through `sync_dir`, so it would otherwise be asserted by nothing until a Windows CI leg ran.

**The two callers weigh the middle answer differently, and that is one policy with a boundary rather than two rules.** `state` treats a flush the platform never performed as durability not confirmed: `CommittedDurabilityUncertain`, which is literally what it is, and an outcome the replacement protocol already carries — no new arm is invented for it. The rename is visible either way, so nothing is withheld beyond the guarantee. `revisions` publishes and carries the weakening in `Published::bundle_durability`, keeping D-121's refusal for a flush that was *attempted and failed*. The dividing question in both modules is the same: is this ambiguous evidence about one directory, which a rebuild can resolve, or a permanent property of the volume, which no rebuild will ever change? See D-121 for the second half of this statement and for what proceeding costs a user on such a volume.

The Phase 12 Windows packaged smoke test is where this stops being an observation on a CI runner's volume and becomes one on a user-shaped installation, alongside the rename-semantics claims `state::replace` and `revisions` already carry there (docs/technical-design.md, "Verification architecture").

### Revision reading

Recorded on completion of Phase 3 task 3, the Revision Reader and handle pinning (2026-07-27). These extend D-087 (revisions is the sole bundle I/O owner), D-063 (companion authorization resolves into a trusted handle), and D-066 (retain only the current published revision).

**D-124 — A Revision Reader is a decoded value, never a path**

Opening a published revision produces a `RevisionReader`: a pinned, owned value that serves product-typed documents. Nothing it returns lets a caller reach a bundle file — no bundle root, no absolute path, no accessor keyed by name, on an accessor, on an error variant, or in a `Debug` rendering, which is asserted rather than assumed because a derived `Debug` would print the bundle root into every log line that touched one.

**Naming an entry is not addressing it, and the line falls there.** A refusal carries a `ValidationReport`, which names the bundle-relative entries that are missing, changed, or unaccounted for — `documents/entry-list.json` and the like. That is diagnosis: it says which part of a revision is wrong, and hands over no root to join it against and no operation that would take it. It is also the report's whole purpose (D-118). The manifest stays unexposed for the converse reason: its required-entry map is a machine-readable index a caller could *use*, not a name it can only read. The design's "does not expose its bundle root, JSON filenames, or arbitrary path access" is upheld as the ability it denies, not as a ban on the string appearing in a diagnostic — and the gate test states both halves rather than only the one that is easy to assert.

It wraps the publication protocol's validator rather than restating it (D-118), so "is this a bundle, and is it *this* revision's" has one answer path for the writer, the reader, and Phase 9's Validate Published Revision. The whole `ValidationReport` travels on the refusal, because those diagnostics read the findings and a flattened summary would force them to open a second, weaker path to get them back. The two answers that are not damage are lifted out of the report: an incompatible schema says rebuild rather than diagnose, and an absent bundle says build rather than investigate.

**Opening proves integrity; only reading proves readability, and the two are deliberately not merged.** Validation hashes bytes without parsing them, so a bundle that is intact and undecodable publishes cleanly — the revision-bundle spike's actual defect. Documents are therefore decoded at the accessor, and every accessor returns `Result<Option<T>, _>`: `None` means the manifest names no such document, `Ok(Some(empty))` means it names one that documents nothing, and the error means it names one this build cannot produce. Deciding that three-way distinction once, against the manifest's required-entry map, is what makes Phase 7's per-language search index — which may legitimately never have been materialized — the same shape rather than a second convention.

Decoding at the accessor rather than at open is the choice against "holding a reader is evidence the revision is readable", which would be the stronger guarantee. It does not survive the scale: a revision is 712–1,475 documents plus a search index from Phase 6 on, and a companion request opens a reader per request. The bytes are re-hashed against the manifest at each read, because with reads deferred the open-time proof is stale by then, and that is what keeps "somebody edited an immutable artifact" a different answer from "this build cannot understand its own format".

The reader is owned and `Send + Sync + 'static` rather than borrowing its store, which is the one place `PublicationCapability`'s borrowing shape does not carry over. A companion request is served on a worker thread with no access to the frame that opened the reader, so a lifetime parameter would put the reader out of reach of the code that reads it — and would infect the companion and desktop handle types that wrap it and everything returning through them. What is shared is the pin registry, not the store: the reader gets the one thing whose lifetime it depends on, not the ability to publish.

**D-125 — A pin is a count in memory, never an open file**

An open reader pins its bundle through an entry in a process-local registry, and every document read is read-and-close. It is not a held `File`, and the reason is platform-specific and load-bearing: on Windows a rename or delete of a directory tree is refused with a sharing violation while a handle inside it is open. A reader holding one would obstruct the publication and the cleanup this pin exists to coordinate with, through the filesystem, where neither party could see the refusal coming or say why it happened. This closes the caveat Phase 3A recorded as open — "publication is never blocked by a reader" stays free rather than becoming a platform-specific hope.

The cost is that a pin binds this process and nothing else: another process, or a person with a shell, can delete a pinned bundle. That one Desktop Host owns one application-data directory (D-065) is the compensating control, and it is a control over who else runs rather than a lock on the directory. It is stated on the module rather than left to be discovered.

Release is a `Drop` — the crate's first — because "eligibility begins only after the last handle releases" has to hold on every path a reader's owner can leave by, including an early `?` and an unwinding panic. An explicit `release()` would be true only by discipline, and the symptom of the first path that forgets it is retention silently never deleting a bundle, invisible until a disk fills.

**D-126 — Retirement is claimed, not checked**

The primitive Phase 9's retention sweep drives is `claim_for_retirement`, which grants an RAII claim only when no reader holds the revision, and refuses every open of that revision while the claim is outstanding. It is deliberately not `is_pinned() -> bool`: check-then-delete leaves a window exactly one open wide, and in that window the deletion *succeeds* — on Windows too, precisely because no operating-system handle is held — leaving a live reader serving from a directory that is going away. Pinning and retiring are therefore one enum in one map, each refusing the other, and both decided under one lock against one entry.

The claim is held across the deletion rather than the lock being held across it. Running the deletion inside a closure while the registry is locked would close the same window, but would put `remove_dir_all` of a whole revision inside a mutex every documentation read must take, so cleanup would stall reads of unrelated revisions.

**Opening consults the registry before the filesystem**, which is what makes the ordering observable: a sweep holds its claim across the deletion, so mid-deletion is exactly when a concurrent open arrives and the directory may be half or wholly gone. The answer must be "this is being removed, resolve a newer revision" rather than the directory's state.

This ships the claim and no sweep, and the two remaining halves are not oversights. Nothing about `staging/` belongs here: that sweep's precondition — state recovery resolved, and every retained manifest having contributed to a complete live set — names facts `revisions` cannot observe. Nor does the claim decide retirement *eligibility*, whose second condition is also outside this module: the previous bundle becomes eligible only after the replacement pointer is observed committed (D-090). It answers the half `revisions` owns, and its name says which half.

**Two things the claim does not exclude, recorded now so Phase 9 does not inherit them as assumptions.** The exclusion is claim-against-reader, not claim-against-writer: publication never consults the registry, so a rebuild whose candidate re-derives a claimed revision's identifier — reachable when a user reverts mod content — adopts the complete bundle it finds at the final path and commits the pointer to a directory a sweep is deleting. And abandoning a claim is safe for the registry, which holds a permission rather than a record of work, but weaker on disk: a sweep that stops partway leaves a corpse the next open classifies as damage — diagnose this — when the truthful answer is absence — build it again. Deletion order, a tombstone, or extending the exclusion to the writer are Phase 9's to choose between.

### Acceptance harness

Recorded on completion of Phase 3 task 9, the acceptance-harness skeleton (2026-07-28). These extend D-094 (verify through fixture builds and published reads) and D-049 (the five golden cases), and are constrained by Phase 6's entry conditions, which delete the seam the harness currently publishes through.

**D-127 — The acceptance harness is an integration-test target that boots the way the app boots**

It lives at `src-tauri/tests/acceptance/`, one target whose sibling files are its modules, and not in a `#[cfg(test)]` module inside `application`. Compiling from outside the crate is itself an assertion: the acceptance path is reachable through the same public surface a transport has, where a module inside `application` could reach private items no desktop read can. The cost is real and is the whole layout constraint — a sibling `tests/*.rs` file is a separate crate and cannot see the harness, so Phase 4 task 8's drift-checked local-corpus run and Phase 11's companion reads must be modules of this target rather than files beside it.

It opens its stores through `composition::open_stores` over a `testsupport::TempAppData` directory. The predecessor — the `mod thread` block absorbed out of `application/host.rs` — called `StateStore::open` and `RevisionStore::open` itself, which restated the revisions-beside-state adjacency that publication's atomic move depends on, and therefore verified a startup path the application does not run. The application-data directory is a subdirectory that does not yet exist, so `create_dir_all` is exercised rather than made a no-op, and the report is asserted: `FirstLaunch` on boot and `Loaded` on restart is the only mechanism by which a leaked or shared directory becomes visible instead of quietly serving somebody else's revision.

**Where it stops is a narrowing, not a shortcut, and it leaves a real gap.** The harness drives `DocumentationHost`, the value the composition root manages and hands to every transport; the Tauri command above it adds `spawn_blocking` and the envelope projection and is not constructible without an `App`. Combined with STE-19's window, which exercises the read while the `seed-skeleton` example drives publication, the consequence is that after Phase 3 closes no Tauri command is invoked end to end by anything. The envelope and DTO shapes are pinned separately by `transport`'s contract vectors; packaged smoke tests in Phase 12 close the rest.

**D-128 — A corpus carries the documentation it stands in for, and lends its snapshots**

The hand-authored documented content is a field of the corpus rather than a parameter of `AcceptanceThread::boot`. Phase 6 deletes the candidate seam, and a harness whose cases *passed in* the content would have every call site change on that day — which is the failure mode "widen instead of replacing" names. As a corpus field it is a field removal, and no case mentions it. The framing is also the one that becomes true: after Phase 6 a corpus really is the sole input, and the field is the stand-in for what `analysis` will derive from its bytes.

That stand-in is the one thing the harness must not let a reader misread, because until Phase 6 **a corpus's snapshot bytes reach a published revision only as its two `RevisionInputs` fingerprints** — nothing parses them, and an identifier appearing both in a fixture file and in an entry summary is the author typing it twice. (The corpus contributes more than the bytes: its mod root is half the derivation of the Mod Installation identifier, and its documentation field is what gets published. The claim is about the bytes alone.)

A doc comment alone would survive Phase 6 and start lying, so the claim is asserted rather than written down, and getting it asserted took two attempts worth recording. The first version compared two separately booted threads, and a negative control — making `AcceptanceCorpus::inputs` ignore its snapshots — left it green: comparing two corpus values proves they differ from each other, not that either reaches a revision, and a cross-thread revision comparison proves nothing either, because two threads mint two Discovery Location identifiers and their installations differ before a fingerprint is consulted. **What makes the comparison mean something is holding the installation fixed**, so the harness gained `rebuild_over`: one thread, rebuilt over a second corpus, asserting that the two revision identifiers differ and the served entries do not. Both halves are then observable, and the control that had passed now fails. The revision-identifier assertion is load-bearing and is not to be removed as confounded; the confound was the two-thread shape it replaced. Only the served-entries half is scheduled to go red in Phase 6.

**Corpus accessors lend `&SourceSnapshot` rather than yielding it.** `source::snapshot::establish` produces a `LiveSource` that exposes only a borrow, and `SourceSnapshot` is not `Clone` because it holds a mutex of captured asset reads — so a by-value accessor would be exactly the memory-backed assumption that forks the harness when Phase 4 task 8 points a run at an installed Vanilla and ACOT. Borrowing lets that phase make the backing an enum inside the corpus module and change nothing else.

### Golden fixtures

Recorded 2026-07-29 on completion of Phase 4K (STE-32), golden fixture authoring. These resolve the implementation plan's open-decision row "Pinned ordinary drawable vanilla technology" and extend D-008, which pinned golden case 2's subject the same way.

**D-132 — Golden case 1's ordinary drawable technology is `tech_xeno_relations`**

Selected by surveying all 355 base-game technologies in the installed `Pegasus v4.4.6` (`modsCompatibilityVersion 4.4`) against every property [`docs/mvp-acceptance.md`](./mvp-acceptance.md)'s golden case 1 must later assert. Eighty-three carried all of them; the tie-breakers below are what narrowed those.

| Case 1 requirement | What the technology supplies |
| --- | --- |
| Nonzero Draw Weight | `weight = @tier3weight1` — nonzero, and reached through the implemented scripted-constants row rather than stated as a literal |
| Resolved base cost | `cost = @tier3cost2` — likewise a constant, so "resolved base value" is a real resolution and not a copy |
| Prerequisite technologies | two, `tech_xeno_diplomacy` and `tech_galactic_administration`, so the prerequisite view is exercised beyond a single edge |
| Eligibility requirements and blockers | `potential = { is_regular_empire }`, a scripted trigger whose own body is an `OR` over `is_country_type` plus a `NOT` over `has_ethic` — both halves, and reached through the scripted-triggers row rather than sitting inline |
| Base Draw Weight and conditional Weight Modifiers | eight modifiers over `has_ethic`, `has_civic`, and `is_galactic_community_member` |
| Content unlocked | `building_grand_embassy` names it by key, and the `buildings` row has no open cells |
| Technology icon rendering | `gfx/interface/icons/technologies/tech_xeno_relations.dds` exists |
| Multi-language localization | name and `_desc` defined in all ten shipped languages |
| Bounded Source Excerpts | 55 lines, well inside the 16 KiB bound |

**The tie-breaker that decided it is the absence of a `factor = 0` clause.** Case 1's subject must be *ordinary*, and a technology that also carried a conditional `×0` would make the ordinary case simultaneously a zero-weight case, blurring the one distinction golden case 2 exists to prove. That is what rejected the otherwise stronger runner-up, `tech_robotic_workers`: it states its blockers directly in `potential` as a `NOR` rather than behind a scripted trigger, and it unlocks a building *and* a decision, but two of its eight weight modifiers are `factor = 0`.

Two further constraints, both about keeping the oracle observation reproducible: it declares no `host_has_dlc`, so the observation does not depend on which DLC the observing installation owns, and it uses no `technology_swap`, a construct rare enough that building the ordinary path on it would be building on the exception.

The properties above are facts about the installed build, not about a committed fixture. Golden case 1 is therefore reached through the drift-checked local-corpus run (Phase 4 task 8) rather than through `fixtures/resolver/`; a re-capture under a new game build is what re-checks them, and a changed result blocks the version update the same way every other oracle claim does.

**D-133 — A golden-case fixture that no record anchors is asserted separately from the oracle suite**

Golden cases 2, 3, and 4 describe *shapes* a mod corpus can have — a conditional `×0` modifier, a zero base Draw Weight granted from two enclosing actions, a corpus that partly fails to parse. None needs an observation of the game to be worth committing, and none has one. So their expectations live in `src-tauri/src/analysis/resolver/golden.rs` rather than in `oracle/`, whose every claim is anchored to a run under `docs/spikes/oracle-records/` and gated against the pinned build. Filing them under `oracle` would imply an anchor that does not exist. Golden case 5 stays in `oracle/` because it genuinely has one (`r1`, `r4`, `r10`).

**Two seams these fixtures deliberately do not assert across, stated because a fixture committed with no expectation is indistinguishable from one nobody reads.** Drawability is not modeled: a `factor = 0` clause is a container the technologies row preserved, and that it *means* "will not appear through normal random research" is golden case 2's product claim, owned by Phase 6. Grants are not modeled either — `ReferenceKind` is a closed four-variant set with no grant among them, nothing walks an event body looking for one, and no reverse index leads from a technology to the actions that award it — so golden case 3's two enclosing actions are asserted as structures present in resolved event bodies, not as routes derived from them.

**The Enigmalith megastructure is asserted twice, and needs both halves.** The megastructures row refuses on its open `FieldRule` cell before reading a file, so the refusal alone would be equally green over a corpus containing no megastructure at all. The suite therefore also parses the file at the parser seam and asserts the definition is present and Clean. One half proves the content exists; the other proves the row is honest about not yet interpreting it.

**D-134 — The acceptance harness runs the golden-case corpora, and cannot assert them**

Each fixture is also a named `AcceptanceCorpus` constructor, so the standard thread runs over it now and Phase 6 does not have to introduce a corpus on the day bytes stop being inert. What those cases assert is thin on purpose: the corpus is a distinct observation, its build publishes, and the read serves the revision back. `analysis::parser` and `analysis::resolver` are crate-private, so an integration target cannot parse or resolve anything — which D-128's honesty control already asserts from the other direction — and a case there that appeared to claim more would be claiming a seam it cannot see.

These are the first constructors in that target to read committed bytes through `include_bytes!` rather than inline literals, which the Phase 3 corpora deliberately avoided for `fixtures/oracle/*`. That reasoning does not carry: the oracle fixtures are frozen because every captured record's manifest pins their SHA-256, `fixtures/resolver/*` are pinned by nothing, and no case in the target depends on a revision identifier's value — only on two of them differing. The alternative would have been a corpus whose bytes were a paraphrase of the fixture, sharing its name and not its content.

The file lists are consequently stated twice, once in `resolver::trial` and once in the acceptance target, because the first is crate-private. `corpora::every_committed_fixture_file_reaches_a_corpus` walks the committed tree and compares, making the directory the authority and both tables derivations of it. Without it the failure that hides is a fixture file added for one suite and silently skipped by the other, since every corpus still builds and every case still passes.

**D-135 — The effective desktop language, and the first additive state field**

Recorded 2026-07-30 on completion of Phase 5F (STE-39). It records the shapes D-097's derivation needed; nothing in D-097 changes. Numbering note: this file already contains two entries numbered **D-132**, which is a defect in the log rather than in any decision, and is deliberately not renumbered here.

**Six detection outcomes, and why five of them are not one.** `DetectedGameLanguage` distinguishes `Detected`, `SettingsAbsent`, `AccessDenied`, `Unreadable`, `LanguageUnset`, and `Unrecognized`. The *behavioural* distinction is two-way: only `AccessDenied` earns the non-blocking notice the design requires, and the other five collapse to English in one arm of the derivation. The *diagnostic* distinction is six-way, and the split is what keeps three claims separable that the design forbids conflating — a file that is not there, a file that says nothing about language, and a file this build is not permitted to read. `Unreadable` carries `io::ErrorKind` for the reason `source::RootError::Unreadable` does, and — for the reason `RejectionReason::code` gives — neither the kind nor either `detail` may enter an identity or a digest. `AccessDenied` is its own variant rather than a `kind` a caller matches on, because leaving it as `matches!(kind, PermissionDenied)` would spread the one distinction the design mandates across every consumer.

`language=""` is `Unrecognized { raw: "" }` rather than `LanguageUnset`: the file stated a value, and that is what tells "the game never chose" from "the game's choice is unreadable". This is a deliberate departure from `analysis::corpora::env_path`'s blank-counts-as-unset rule, where a blank environment variable was a shell accident with `/` as the hazard; here both fall back to English, so preserving the evidence costs nothing.

**The effective language carries provenance and carries detection whole.** An override of `l_english`, a detected `l_english`, and the English fallback are one tag and three situations, so `EffectiveLanguage` carries a `LanguageSource` for the reason the resolver's `EffectiveField` carries a `FactKind` — without it, the derivation-order requirement is not observable from the result. The access condition is a separate accessor rather than a `LanguageSource` variant because the design shows the notice *even when an override decided the language*; folding it into the source would let an override silently erase it. `EffectiveLanguage`'s fields are private with one constructor, because two of the states the shape can spell — an `ExplicitOverride` with no override, an `EnglishFallback` beside a detected language — are nonsense, and the derivation is the invariant.

**A narrow targeted read; the Jomini adapter rejected.** `settings.txt` is scanned by a byte state machine that extracts one top-level `language` assignment and understands nothing else. Reuse was not available and would have been wrong if it were: `analysis::parser` is private, `parse` takes a `SourceIdentity` no caller has here, and the dialect lexer carries a corpus-conformance obligation (src-tauri/AGENTS.md), so making startup language detection its second consumer would let a lexer change break the language the app boots in without the `--ignored` conformance run observing it. Three rules carry the correctness and each has its own negative control: a whole-token key match (the real file's `soundgroup="l_english"` can disagree with its `language=` line, so a substring match returns a plausible wrong answer rather than an obvious failure), a depth-zero guard (the real file has a `graphics={ … }` block), and first-one-wins. **The gap:** no oracle record settles what the game's own settings reader does with two top-level `language` assignments; a capture over a hand-duplicated `settings.txt` is what would settle it, and until then the tie-break is pinned as arbitrary rather than presented as behaviour.

**`CURRENT_SCHEMA` stays 1, and what the first bump owes.** The override is an optional field with a read-side default, so a document written before it existed decodes unchanged and a build without the field ignores the key. Bumping would have been actively harmful in both directions: with no schema-1 arm at `store::open_existing`'s dispatch, every existing document falls to `quarantine` — losing Discovery Locations and publication references to add a preference — and an older build reading a bumped document stops at the blocking newer-schema screen for a field it could have safely ignored, destroying rollback for nothing. With an arm, the arm's body is an identity function. The criterion for the first real bump: the first field whose absence a reader cannot correctly default, arriving together with the older-schema arm the comment at that dispatch reserves.

STE-39's fourth acceptance criterion reads "the state-schema addition for the override follows the normal migration path", which taken literally asks for that bump. It is recorded here as reinterpreted rather than silently reread: the normal path for a document that still parses into the current type is the one where nothing migrates, and what the criterion is owed — a read-side default, absent-field decode, encode round-trip, and schema dispatch left provably correct — is asserted by `state::model`'s two extended tests, its new `encode` round-trip, and `store::a_document_written_before_the_override_existed_still_loads`, which is also the control that goes red if anyone bumps the constant without adding the arm.

**Leniency on the persisted override, as a departure from `DiscoveryLocationId`.** An unreadable `language_override` decodes as absent instead of failing the document. An identifier is machine-generated and a malformed one is genuine corruption, so `corrupted_state_deserialize_error_names_the_offending_value` is right to let it quarantine; a language name is the one field in the document a curious person would hand-edit, and `"english"` for `"l_english"` must not cost them their publication references, their orphan-cleanup eligibility, and a recovery screen. A non-string is still a decode error: the leniency is about the value, not the type. `LanguageTag` therefore derives `Serialize` and not `Deserialize` — the lenient decoder is the only decode site, and a strict impl would exist solely to be a trap. Deferred alternative: carry the rejected text so Phase 10 can say "'english' was not recognized"; it needs a screen that displays it before it is worth persisting.

**One vocabulary for a Stellaris language, owned by `localization`.** `LanguageTag` parses `l_<name>` syntactically rather than against the ten languages the shipped game has, because a translation mod adds a language and an allow-list would classify every community translation as corrupt — and because the design requires an effective language no revision can serve to still *be* the effective language, repaired per key downstream. It is bounded at 64 bytes so a corrupt settings file cannot make a megabyte of bytes into a durable preference. **Merge rule with STE-37**, which lands `.yml`-header locale identity concurrently: one type, and `localization` owns it. Whichever of the two merges second performs the collapse — if STE-37 is second, its ingestion parses headers through `LanguageTag`; if STE-39 is second, `language.rs` folds into STE-37's type and takes `english()` and the length bound with it. `LanguageTag` is a durable state type from this ticket on, so any later narrowing of the parse is a state-evolution question rather than a refactor: it would reclassify persisted values as unreadable, and the lenient decoder bounds that to "the preference resets".

**Two edges, and neither is a new peer edge.** `state -> localization` for the override's type. Owner: `localization`, which owns the `l_<name>` vocabulary. Cycle check: `localization` depends on `canonical` and `error` and never on `state`; `analysis -> localization` runs the other way and `analysis` has no path to `state`. It is not added to the sanctioned peer edge list, on D-122's own stated reasoning for the unlisted `state -> discovery`: `state` sits in the row above the deep modules, so its edge is downward rather than peer-to-peer. `localization` acquires **no** edge to `discovery`: the Stellaris user-data root is named once in `discovery::proposals`, beside the launcher-settings read for the reason that file's doc comment gives, and detection takes the path as a parameter.

**No cache, no startup wiring, no wire projection.** The derivation reads the settings file on every call and holds nothing, which is the design's requirement rather than an omission — a remembered detected language would be the second authority "refreshed … rather than copied into the mutable-state authority" forbids, and it would also make the game-language-change test an assertion about our own cell. "At startup and explicit Refresh" is consequently a staleness bound met vacuously, and it is not wired: Refresh does not exist (Phase 9), `run()`'s setup closure is unreachable from any test, and `discovery::proposals` is the standing precedent for platform detection shipped tested and unwired ahead of the Phase 10 wizard that consumes it. Phase 10 inherits this checklist: a `DocumentationHost` method (which needs `home` in `new`'s signature), exhaustive `Serialize` projections for the three types with no `_` arm, the hand-mirrored TypeScript union, the `never`-guarded refusal switch, one `fixtures/transport/` vector per variant, both halves of the contract suite, a `TAURI_API_IMPORTERS` entry, and a redaction decision for `AccessDenied::detail` and `Unreadable::detail`, which carry host `io::Error` text the wire contract does not admit.

**The first `#[allow(clippy::…)]` in the crate, and why it is not a precedent for suppression.** Adding a 24-byte field to `AppState` pushed `state::store::OpenOutcome` over `clippy::large_enum_variant`'s threshold, because its `Ready` variant carries the whole store. The lint's premise is an enum copied often; this one is constructed once per process, moved twice, and destructured immediately. Boxing the store would trade a smaller move for an allocation and a pointer indirection on every state lock for the life of the process, so the suppression sits on the enum with that reasoning rather than the design being bent to satisfy a heuristic. Any further suppression owes the same argument at the site.

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

**D-130 — Reference visibility is fact-scoped; registry `Pending` stays reserved for policy-unknown cells**

Recorded 2026-07-28 on completion of Phase 4E (STE-26), the technologies row and golden case 5. It refines the references cell D-098 requires of every row; nothing else in D-098 changes.

**The problem the row surfaced.** Technology bodies carry `@` constant references and `inline_script` inclusions — `r1-target` proves the game resolves a vanilla `@tier5cost3` read from a mod file — while the rows that own those references (scripted constants, inline scripts) are separate tickets with separate evidence. The references cell had one variant, `NoReferences`, whose contract is that a row carrying references states `Pending` instead. But an unresolved cell refuses the whole registry, so under that reading the technologies row could not resolve at all, and the phase's named exit condition could not be met by any implementation that was also honest.

**The distinction that resolves it.** A `Pending` cell means *nobody knows the policy*: no oracle record settles what the game does, so the resolver must refuse rather than guess. That is not this situation. The policy here is known — the game resolves these references — and what is missing is the implementation, in a ticket that already exists. Those are different failures and they deserve different mechanisms. Conflating them would make `Pending` mean "unimplemented" as well as "unknown", and the cell would stop being able to say which.

So the references cell became per reference kind, each kind carrying its own handling, and `DetectedNotResolved` is the handling the engine implements now: the reference is found and recorded as a fact against the effective field, and the value keeps its reference text. Per kind rather than one verdict per row so that Phase 4F and Phase 4G are each a one-kind status change rather than a rewrite of the vocabulary. `ReferenceRule`'s existing rule is upheld rather than weakened: a name is offered only once the engine honours it, and `DetectedNotResolved` exists because detection exists.

**A kind the row did not declare refuses.** The kinds list is a claim in both directions, so an undeclared kind found in a body is `Refusal::UndeclaredReferenceKind` — the mirror of `UndeclaredFactKind`. Without it, `kinds: &[]` would mean both "carries none" and "carries some, unnoticed", and the second is exactly the silent incompleteness the cell exists to prevent: a `cost` of `@tier5cost3` published as though it were a literal value.

**Detection reads the parser's decision rather than making one.** `ScalarKind::VariableRef` and `VariableExpr` are assigned by the dialect lexer. The resolver matches on them instead of re-deriving "what is an `@`" from raw bytes, so there is one authority for what a reference token is. Detection runs over effective fields after the repeat rule has decided, because a reference in a definition that lost never reached the answer.

**What this does not do.** It does not resolve a reference, does not pull Phase 4F or 4G forward, and does not reorder tickets. The two deferred behaviours are pinned by tests scheduled to go red in STE-27 and STE-28, so the flip is a decision somebody makes rather than a behaviour that quietly arrives.

**D-131 — An inline-script shape no oracle record measures is a typed omission, never a guessed expansion**

Recorded 2026-07-29 on completion of Phase 4G (STE-28), inline-script expansion. It records two policies chosen for shapes `r11` and `r12` do not cover; nothing in D-098 or D-130 changes.

**What the records do settle.** `r11-inline` measured six subjects through a real game start: a simple inclusion expands, `$PARAM$` substitutes, an inclusion nests and must be expanded recursively, and a mod file at a vanilla inline script's path replaces its content. `r12-inline-missing` measured the failure: the game names the consuming file and line, and the technology **still registers** with the included content simply absent. So expansion is implemented, and a failure omits one inclusion rather than refusing the definition.

**The problem the mechanism surfaced.** Two shapes reach the expander that neither record touches. A fragment can carry a `[[PARAM] … ]` conditional block — the dialect parses them, and nothing measures whether or on what condition the game compiles one inside an inline script. And a fragment can use a `$PARAM$` the call never binds.

**Both are typed-unresolved, with the inclusion omitted.** A conditional block is `UnresolvedInline::ConditionalUnmeasured` and an unbound parameter is `UnresolvedInline::UnboundParameter { name }`; in each case the whole inclusion is absent from the effective field and the site carries the reason. The alternatives were each a guess wearing an answer's clothes. Compiling a conditional on a documented-but-unmeasured reading would put content into a technology page on the strength of a wiki, not a measurement — and omitting only the conditional half would be a second guess, about which half the game keeps. Substituting an unbound parameter with an empty value would fabricate content: `factor = $F$` would become a factor of nothing. Leaving the `$PARAM$` token in place would be worse still, because the technologies row does not declare the `Parameter` reference kind, so one unbound parameter three fragments deep would refuse the entire registry — a blast radius the game itself does not have.

**Why omission is the right failure and silence is not.** `r12` is the authority for the shape: the definition survives, structurally valid, with the inclusion missing. The resolver owes the same survival. What it must not inherit is the quietness — "failed to expand" and "there was nothing to expand" are the same silence, and it is the silence, not the failure, that would publish a technology page missing its weight logic with nothing anywhere to reveal it.

**What would settle each.** A capture exercising a `[[PARAM] … ]` conditional inside a fragment, with and without the parameter bound, would replace `ConditionalUnmeasured` with a measured rule. A capture supplying a fragment with a parameter the call omits would do the same for `UnboundParameter`. Until then each variant names its own gap, the same discipline `UnresolvedConstant` follows.

**Scope.** Technologies only. `scripted-triggers` and `scripted-effects` keep `DetectedNotResolved` for this kind: `r11` measured a technology consumer, and per-row evidence is what a row may declare from.

**D-132 — An embedded `$PARAM$` earns a typed omission, not silence; the corpus says the silence costs nothing yet**

Recorded 2026-07-29 on completion of STE-34, the embedded-parameter census. It settles the shape D-131 named but did not cover. Nothing in D-131 changes.

**What was unmeasured.** The lexer calls a token `ScalarKind::Parameter` only when `$` is the first and last byte, so `tech_$TIER$` is an ordinary `Unquoted` scalar, `substitute` never touches it, and it reaches an effective field as literal text with no fact attached. Unlike D-131's two shapes, this one was not detected at all — the single place the inline-script mechanism was silent rather than typed.

**What the census measured.** [Inline-script parameter census](./spikes/inline-parameter-census.md), reproducible as `embedded_parameter_census`. Across 624 surviving fragments of Vanilla `Pegasus v4.4.6 (fdde)` and ACOT `1419304439`: 789 embedded occurrences (657 `Unquoted`, 69 `Quoted`, 63 inside `@` references) against 1,469 whole-token ones. The shape is real and abundant. But the technologies row — the only row that expands inline scripts — reaches exactly four fragments across its 1,436 definitions and 158 inclusion sites, and those four hold one parameter between them: the whole-token `$TECHNOLOGY$` that `r11` measured. Zero embedded. Zero conditional blocks, anywhere in the fragment corpus, which makes D-131's `ConditionalUnmeasured` vacuous by the same measurement.

**The decision.** `UnresolvedInline` gains `EmbeddedParameterUnmeasured { token }`: an inclusion whose fragment still holds a closed `$…$` run in a token the lexer did not classify as `Parameter` is omitted, and the site carries that reason. This is D-131's rule applied without an exception — the alternatives are the same two it already rejected. Substituting textually would put content into a technology page on a rule no record measures, and the corpus's own intent is not a measurement. Leaving the token in place would publish `has_technology = tech_$TIER$`, an identifier naming nothing, and would do it *invisibly*: the undeclared-kind refusal cannot catch it either, because `Scan::walk_scalar` reads `ScalarKind` too, so an embedded run in an `Unquoted` token is outside its reach exactly as it is outside `substitute`'s.

**Detection does not wait for a record; substitution does.** The variant is the honest omission, and D-130 already draws this line — `DetectedNotResolved` exists "because detection exists". So `r19-inline-embedded` (drafted in the note) is what would let the resolver *substitute* an embedded parameter, not what permits it to notice one. This is a deliberate reading of STE-34's "implemented only with a record behind it": that condition governs the handling, not the detection.

**Why the code does not change here.** STE-34 is a measurement whose stated scope touches no resolver code, and the census is what makes the interval safe rather than merely unexamined: no reachable fragment carries the shape, and a committed run fails the moment one does. The same run's negative control has been shown to go red on exactly that condition. Adding the variant is a follow-up ticket, not a thing to slip into a spike.

**Scope.** The variant is vocabulary shared by every row that expands, so it is not per row. What is per row is exposure: `scripted-triggers`, `scripted-effects`, and the Phase 4H rows hold `InlineScript` at `DetectedNotResolved` and reach no fragment today. The 65 distinct embedded occurrences under `ship_components` and 119 under `grand_archive` are what one of those cells flipping would walk into, which is the reason the variant is specified now instead of being rediscovered then.

**D-136 — Display tokens prove the form of markup, never its knownness, and account for every character they cover**

Recorded 2026-07-30 on completion of STE-38, the markup tokenizer. D-041 fixes which markup the *renderer* supports; this records the vocabulary that carries it, which D-041 does not describe and every later Phase 5 task and the Phase 10 renderer inherit. Nothing in D-041 changes. Numbering note: STE-38 was authored against a log whose highest entry was D-134 and reserved D-135, which STE-39 merged first; this entry is the renumber, and the duplicate **D-132** D-135 records is untouched here for the same reason it gave.

**Form, not knownness.** A `§` code is accepted when it is one character from `[A-Za-z0-9_]`, not when it names a colour the game defines. `interface/fonts.gfx` declares `bitmapfonts.textcolors` as a data table — 30 single-character keys in `Pegasus v4.4.6 (fdde)`, of which vanilla text uses 29 — and two of the workshop mods installed here redefine it. An allowlist derived from vanilla would therefore be wrong for vanilla *and* would render a mod's own code as garbage. The same reasoning extends to icon names and localization keys, which mods likewise define: the tokenizer proves shape and leaves existence to the phase holding the table. A consequence worth stating, because it looks like a gap: `TokenKind::Reference` cannot promise a Static Localization Reference. `$VALUE$` and `$ORD$` are syntactically identical to real keys, so resolution deciding a key is absent is ordinary fallback to raw (ADR 0004) and not an Analysis Issue — treating it as one would manufacture false unresolved references in the hundreds of thousands.

The rule is the module's stance rather than this ticket's preference, which is worth recording because the two tickets reached it independently and a day apart. D-135 parses `LanguageTag` as `l_<name>` syntactically rather than against the ten shipped languages, on the argument that a translation mod adds a language and an allow-list would classify every community translation as corrupt. That is this paragraph's argument with a different table under it. Anything in `localization` that validates against a set vanilla happens to ship is suspect for the same reason, and the burden falls on whoever proposes one.

**Style runs are flat, not nested.** `§!` returns to the default style rather than popping an enclosing one, so a renderer holds a current-style register and not a stack. The corpus is unambiguous: across 1,506,561 values, 6,037 re-open the outer colour by hand after an inner reset, 94 open a run and never reset it, 58 reset with nothing open, and 550 carry a doubled `§!§!`. A nested-span model would need an invented repair for each of those four shapes, and none of the four is an error in the game.

**Every token accounts for its slice, and the spans tile the value.** This is how "never dropped and never interpreted" becomes checkable rather than asserted. Spans must be sorted and contiguous from 0 to the value's length, so a dropped character leaves a gap; and each token's span must cut exactly the text re-derived from its own payload, so a token cannot lose a character while the tiling still holds. The two halves catch different faults, and both have been shown to go red — a scanner seeded to discard unpaired markers failed the tiling property, the reassembly property, and four behavioural cases. The invariant is a property of tokenizing **one** value and does not survive reference resolution, which splices text from other keys; that ticket owns its own losslessness story.

**Recovery is one character wide, and `[[` is not an escape.** A marker that begins no recognized construct becomes a one-character verbatim token and scanning resumes on the very next character. So the shipped `"in £energy\u{a0}£Energy Credits"` costs a visible `£` instead of the rest of the sentence, and `"[[$INDEX$] $FLEET_NAME$"` keeps `$FLEET_NAME$` resolvable. `[[` is widely described as an escape for a literal `[`, and nothing measures it, so it is not implemented — D-131 and D-132 set the standard that detection may precede a record while handling may not. A capture of what the game displays for one of the 41 vanilla values containing `[[` is what would settle it, and would change only which token that pair produces.

**Verbatim spans keep a reason even though they render alike.** Concept links, runtime tokens, `$@scripted_variable$`, a `£$ICON$£`, and an unpaired marker are all displayed as authored today, so the arms exist for what outlives rendering. Two carry weight now: classifying a concept link at its boundary and refusing to parse inside makes it *structurally* impossible for resolution to expand the `$display$` in `['key', $display$]`, and the `@` sigil names the scripted-variable namespace, so that form cannot be a Static Localization Reference by definition. A name-system `$1$` gets no arm of its own — its key is well-formed and only convention says otherwise, and inventing a convention-based exception would contradict form-not-knownness.

**Owned text rather than borrowed slices.** Every payload is a `String`. Zero-copy would matter against the 151-to-178 MiB tables, but ADR 0009's shared Localization Store is not built and a revision preserves only cited localization plus its static-reference closure — 1.74 to 2.59 MiB measured — while every consumer needs owned text at its boundary regardless. Nothing measured today justifies putting a lifetime in the interface each later phase inherits. If a future content type balloons preserved localization by orders of magnitude, this is the contained change to revisit.

**What the committed suite does not yet reach.** Verification stops at the case table and a property test over generated marker soup. The natural companion — tokenizing every installed value and asserting the gate on each — cannot land here, because reading `.yml` files needs ingestion and a test-only reader would become the second authority the parser and resolver both refuse to grow. A throwaway harness was run against the installed tree during implementation and found no faulting value in 1,506,561 vanilla values or 324,564 workshop ones, but that run is not reproducible from the repository and is therefore evidence for the design rather than a gate. STE-43's drift-checked local-corpus localization run is where it becomes one.

**What is not versioned yet.** `localization_interpretation` stays at 1. Phase 5 is its first implementation and nothing has been published under any tokenization semantics. Once revisions exist it becomes a build-input semantic — it feeds the search projection and the cited-key closure, which decide what a bundle stores — so a later change to these marker rules must bump it.

## Open decisions

**Q-002 — Project identity**

The project name and copyright-holder wording are not yet chosen. Create the root `LICENSE` file after those are known.

**Q-004 — Remaining Resolution Profile cells**

Pre-implementation oracle investigation is complete, but the Resolution Profile remains partial. Implement the resolved rows first and use resolver conformance traces plus focused oracle fixtures to close the remaining cells. An unresolved cell blocks support for the content type that requires it, not implementation of the resolver module or unrelated resolved content types.

See [Resolver evaluation](./spikes/resolver-evaluation.md).
