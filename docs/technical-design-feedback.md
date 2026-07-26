# Adversarial review of `technical-design.md`

Status: Applied — retained for traceability

Last updated: 2026-07-25

This document records the findings from the second adversarial review that were accepted for action after deduplication and disagreement review. They have now been incorporated into the [technical design](./technical-design.md), supporting decisions, acceptance criteria, and focused spikes. The findings remain below as the review record rather than as a current gap list.

## Original verdict

**Approve the architectural direction; resolve the blocking findings below before accepting the design as final.** The documentation changes needed to resolve those findings were applied on 2026-07-25; the design remains in progress until its explicitly planned evidence spikes are completed.

At review time, the design had absorbed the majority of both reviews. What remained concentrated in the semantic center: resolver semantics, one recovery-path contradiction, the asset-failure lifecycle, deterministic canonicalization, and the acceptance-corpus reconciliation.

## Originally blocking findings

### 1. Resolver behavior needs a contract and a reproducible game oracle

The design states that resolution policy is content-type-specific and pins one redefinition fixture, but stops there. There is no resolver contract for the two-layer Vanilla-plus-Target-Mod case, and the decision log's own feasibility note records that Stellaris composition is not uniformly file-level or last-wins. Deferring Playset composition does not remove the need to reproduce the two-layer subset correctly. A parser can preserve source perfectly while the resolver still generates authoritative-looking but wrong documentation.

The design needs a content-type resolution matrix covering, at least: file-path shadowing and directory replacement; registry collisions for technologies, megastructures, buildings, ship components, events, scripted triggers, scripted effects, constants, localization keys, and sprite definitions; field inheritance versus whole-definition replacement; duplicate definitions within one source layer; Target Mod precedence and ordering among locally available DLC; localization and asset precedence; scripted-constant scope, redefinition, and cycles; and how provenance records each contributed, inherited, defaulted, or shadowed fact.

The redefinition fixture also needs an oracle protocol to be reproducible: pin the Stellaris build and platform, the installed DLC set, exact fixture source and checksums, the mechanism used to observe effective game behavior, the expected effective fields and provenance, and the maintenance rule when Stellaris updates. "Compare with observed Stellaris behavior" is not yet a procedure. The since-corrected `×0` golden-case misattribution is direct evidence that assumed game behavior cannot be trusted.

### 2. Automatic state reset silently unprotects every revision bundle

State recovery promises that "automatic state reset never deletes Documentation Revision bundles," while startup maintenance removes "unreferenced bundles after proving that no application-state pointer names them." Automatic reset replaces the state with defaults, which removes every publication reference; the next cleanup pass can then legally delete every preserved bundle. The same paragraph concedes the mechanism: "unreferenced bundles remain subject to the separate retention and cleanup policy."

Remedy: quarantine the malformed state file under a diagnostic name rather than overwriting it; start with defaults but mark publication-reference recovery unresolved; suppress orphan revision and asset garbage collection for that startup; and make garbage collection fail conservatively whenever any retained manifest cannot be read — an incomplete live-set calculation must never justify deletion. To make relinking possible at all, the bundle manifest must record its Mod Installation identifier; the manifest fields specified in the revision-bundles section do not include it.

### 3. Asset conversion failures have no owner in the candidate lifecycle

`analysis::build` produces the Revision Candidate; asset materialization runs afterward; and the design promises that missing bytes, malformed media, and conversion failures become Analysis Issues with documented placeholders. No module can legitimately write those outcomes back into the already-produced candidate: if `application` mutates it, it acquires documentation-generation policy; if `revisions` does, persistence acquires domain semantics; if `assets` does, asset mechanics acquire documentation ownership; if nobody does, the promise is unimplementable.

Remedy: an explicit two-phase type boundary —

```text
Analysis Draft
    -> asset materialization outcomes
    -> finalized Revision Candidate
    -> revision publication
```

Finalization is owned by `analysis` (or a narrowly defined domain finalizer). It receives successful asset keys and typed failures, replaces failed logical assets with deterministic placeholders, adds scoped Analysis Issues, computes the final required-key set, and only then yields a publishable candidate. Publication should validate required blobs by content or trusted blob metadata, not path existence — a corrupt file at the right key must not pass.

### 4. Canonicalization and numeric rules are still undefined; route identity cannot close without them

The design now lists stable route identity in Undecided with the right constraints and has expanded the analysis-component version list. Three things still block determinism, which is an ADR-level commitment:

- **Canonicalization rules** for everything that feeds hashes, manifests, stable identities, and reproducibility tests: path separator, case, and Unicode treatment; source enumeration ordering; registry and provenance ordering; map and set serialization; stable ordering of requirements, modifiers, routes, issues, and excerpts; locale-independent search normalization; and what revision identity derives from.
- **Numeric representation:** preserve the original numeric lexeme and use an exact decimal or rational normalized form where game semantics allow — binary floating point makes exact source values, identity, and equality fragile.
- **Closing the route-identity decision** requires defining "same enclosing action" across event options, nested effects, scripted effects, and `immediate` blocks — the same definition "one route card per Grant Site" depends on — plus a derivation (destination Entry Key, canonical Grant Site structural identity, normalized Unlock Effects, Player-Facing Anchor) tested against: whitespace and comment changes; unrelated insertions that shift ranges; semantically neutral reordering; changed requirements; added or removed effects; and a material change that must produce a new identity.

Two version-vector residuals: the expanded list should also formally include source-enumeration policy and the parsed-model schema.

### 5. The acceptance corpus is contradictory

The PRD and D-049 define four golden cases; the standalone acceptance document contains a fifth semantic case, technology redefinition. Update the PRD and D-049 to five — redefinition is now essential resolver evidence rather than an optional unit test — and explicitly incorporate every case into the primary harness.

## Original significant findings

### 6. Startup freshness verification contradicts the persistence principle

The companion access section now defines freshness observations produced at "application startup, opening a Target Mod, explicit Refresh, and successful build publication" and forbids per-request hashing. But the persistence principle still says startup reconstructs the catalog "without parsing or fingerprinting complete Mod Source." If startup is a verification event, its scope and cost are undefined — a full pass hashes Vanilla plus every published mod before readiness is known, seconds each by the design's own measurements. If it is not, revisions stay un-Ready until first open or Refresh and the event list overstates. Choose one: a bounded asynchronous startup verification phase, a scoped pass over published revisions only, or startup removed from the event list with an explicit Unchecked presentation.

### 7. Search input contradicts the client boundary; cancellation is undefined

Host-owned search says React supplies "revision selection," while the stable-address and documentation-client sections forbid revision identifiers from reaching React. The wording originates in D-054, so fix both documents: React supplies a Mod Installation identifier; the host resolves it into an authorized revision handle. Separately, a Tauri invocation has no transport abort, so "cancellation" must be defined as one of: client-side result suppression only, transport abort where supported, or a cooperative host token. Do not include it in the product DTO unless the host actually observes it.

### 8. Analysis Issue propagation has no impact model

The PRD requires mod-level warnings plus narrower warnings "when impact is known," but nothing defines how impact is computed. The model must distinguish evidence that is absent, evidence that is present but unsupported, and facts whose downstream dependents are potentially incomplete. Illustrations: a malformed localization file affects names, not weight semantics; one unparsed scripted trigger should not taint every entry; a failed registry file makes the complete entry set unknowable; an unsupported condition marks its modifier unsupported without silently dropping it. Without this model the app either over-warns until nothing is trusted or under-warns while omitting behavior.

### 9. The crash-safety guarantee overstates the specified protocol

"A failed encode, write, flush, or replacement leaves both the in-memory authoritative value and the last valid persisted file unchanged" is falsifiable: after a successful rename but failed directory synchronization, the file *is* replaced and durability is uncertain — the old file did not survive. The design does not need per-platform state machines; it needs the commit point defined for both state replacement and bundle publication, a named "committed but durability uncertain" outcome, and one rule: on any ambiguous outcome, reopen and validate the state path rather than assume the old file survived. Failure-injection tests are only meaningful once those commit points are normative.

### 10. The single-instance invariant claims more than the mechanism enforces

The section still opens with "One Desktop Host process owns each application-data directory," but the plugin coordinates on the application identifier and never inspects a data directory — a differently identified binary or a custom data-root pointed at the same directory is not excluded. Narrow the invariant to one packaged process per application identifier, holding for the data directory only through the packaged app's fixed identifier-to-directory mapping.

### 11. Path normalization and location rebinding need explicit rules

"Semantics of the host filesystem" under-specifies stable identity. Define: case-sensitive versus case-insensitive comparison; Unicode normalization (APFS is normalization-preserving, so identity must not depend on the filesystem's choice); separator and drive-letter handling; symlink, junction, and reparse-point policy; case-only renames; invalid encodings; and whether logical paths use raw directory entries or canonicalized targets.

Editing a Discovery Location's absolute path while preserving its stable identifier silently re-attaches per-installation preferences — Hidden Routes, last selection — to whatever occupies the same relative paths in the new root. Fingerprints protect revisions from this, but not preferences. The rebind confirmation should state the consequence, and the design should decide whether preferences are retained, quarantined, or reset when the new root appears unrelated.

### 12. "Thrown" failure semantics must be specified as non-panicking

"Thrown" risks conflating a typed unexpected error with a Rust panic. Specify: unexpected failures propagate through a typed internal error channel; no panic crosses a transport boundary; HTTP returns an opaque `500` with a correlation identifier; Tauri rejects with an opaque transport error; detailed chains and absolute paths appear only in protected desktop logs; React has a route or application error boundary; and pairing secrets, session cookies, absolute paths, and source contents are redacted from logs.

### 13. Companion hardening still needs normative lines

Add: at least 128 bits of cryptographically random entropy for both secrets; a concrete pairing-secret expiry duration; constant-time secret comparison; a maximum active-session count; a host-only cookie (omit `Domain`); explicit cookie deletion on disable; `Host` parsed as an authority per RFC 9112 with malformed and duplicate forms rejected — "a `Host` containing one of the host's addresses" is string containment, which is exactly the bypass shape; request header, body, concurrency, and timeout limits; `Cache-Control: private, no-store` for documentation and Source Excerpts if post-session browser-cache visibility is undesirable, and `private, max-age, immutable` for content-hashed assets; no service worker without a separate cache-and-credential review; and a Companion panel warning that sessions travel over unencrypted local HTTP.

### 14. Production CSP needs named release gates

The design requires production CSPs to be defined and verified before release but names no gates. Because localization and generated documentation are external inputs rendered in a WebView and browser, make these explicit: no raw localization HTML; no `dangerouslySetInnerHTML` for source-derived content; no remote script, stylesheet, font, or image origins; no `unsafe-eval`; inline-style handling justified or replaced; the asset protocol permitted only in `img-src` where possible; separately tested CSPs for the Tauri window and the Companion HTTP service; and compatibility tests against the production React build, shadcn, and the chosen diagram renderer. The controlled-token localization model makes a strict policy realistic.

### 15. The revision-bundle spike needs declared thresholds

The spike's acceptance language — "responsive" and "operationally simple" — should be converted into declared budgets before the format decision is recorded: cold open, search latency, record latency, retained memory, file count, validation duration, and bundle-size expansion. Without numbers, any measurement can be argued into acceptance.

## Original minor items

- **Desktop browser-history mechanism.** The design requires path preservation and a packaged test but names no mechanism. Tauri's built-in asset resolver falls back to `index.html` for unknown paths; name that as the mechanism so the test verifies a stated behavior rather than an assumption.
- **Asset-protocol scope.** Name the exact scope entry per platform (an `$APPDATA`-style pattern covering only the Asset Store subtree), and preserve the DDS feasibility artifacts — sample files, converter version, command, output hashes — in the repository.
- **Result package exit.** Add the missing sequencing exit: until the package is published, the application vendors the equivalent type locally, so MVP progress never blocks on extraction. Contract tests should cover every operation and every union variant, not representative examples.

## Requested verification additions

- **Reproducibility.** Build the same corpus in different temporary roots, enumeration orders, and worker schedules; compare canonical documentation and manifest identities.
- **Metamorphic.** Comment and whitespace changes alter no authoritative fact; insertions that only shift byte ranges preserve route identity; an unrelated binary asset alters nothing.
- **Game oracle.** Validate effective definitions against the pinned Stellaris fixture, including the omitted-`potential` redefinition (pairs with finding 1).
- **Completeness propagation.** Inject failures at every parser, resolver, localization, route, and asset stage; assert the exact revision, entry, and section warnings (pairs with finding 8).
- **Crash recovery.** Reopen the application data directory after failure at every persistence step, including ambiguous post-rename and post-pointer states; assert both the visible revision and garbage-collection safety (pairs with findings 2 and 9).
- **Security and paths.** Symlinks, junctions, traversal encodings, percent-decoded separators, malformed and duplicate `Host` headers, invalid `Origin`, oversized queries and bodies, stale cookies, and absolute-path redaction.
- **Status states.** Every combination of availability, freshness, integrity, completeness, build state, and desktop versus companion access.
- **Packaged tests.** Windows and Linux release tests exercise filesystem replacement semantics, single-instance behavior, asset scope, browser-history refresh, firewall behavior, and the exact selected Linux package formats.

## Resolved since the original review

Recorded for traceability; no additional action was requested for these points in this review. At review time, the design already covered: newer-schema fail-closed startup; pairing-secret rotation, atomic consumption, attempt limits, and sequential multi-device pairing; referenced-asset freshness identity and snapshot byte freezing; companion readiness as event-derived observations with no per-request hashing and defined mid-session staleness behavior; Jomini's source-range and failure-isolation gaps recorded at the parser seam with the spike reframed as "extend or wrap"; route identity acknowledged in Undecided with correct constraints; the expanded analysis-component version list; runtime asset garbage collection with a grace period; the macOS single-instance socket mechanism and multi-user caveat; macOS Documents-folder access handling; the Ensure cache-hit "Checking for changes…" presentation; the listener "waiting for first connection" state with troubleshooting; explicit TanStack Router cache configuration; and the enumerated intra-module dependency edges.
