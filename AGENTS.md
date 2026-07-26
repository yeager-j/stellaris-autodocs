# Stellaris Docs

A desktop app that parses Stellaris mods to generate accurate, easy to read documentation.

## Engineering Principles

> _Perfection is lots of little things done well_
>
> — Marco Pierre White

These are portable defaults. Project-specific instructions and observed evidence take precedence.
Apply a principle only where its assumptions hold; do not add machinery merely to satisfy a slogan.

### How to read this document

Not every principle has the same force:

- **Must** marks a correctness, security, or data-integrity requirement. Depart only through an
  explicit exception that names the risk and its compensating control.
- **Default** marks a design choice that usually pays for itself. Contrary local evidence may
  override it.
- **Diagnostic** marks a reason to investigate, not a verdict or an instruction to refactor.

Unless a principle uses `must`, `never`, or equivalent as an instruction, read it as a Default. The
Diagnostics section contains Diagnostics. The purpose of this distinction is to preserve judgment
while making clear which constraints judgment may not silently waive.

### Organizing idea: authority, home, and derivation

Most avoidable complexity comes from a fact, decision, or rule having no clear home, or from several
places acting as its authority. Ask where each becomes knowable, which lifetime it belongs to, which
context can enforce it, and how its consequences reach the rest of the system. Make the choice once
at that home; preserve what was learned at boundaries; derive or synchronize every other
representation explicitly; hide the resulting complexity behind the smallest useful interface.

### Working method

- Before editing, define the intended outcome and how it will be verified. State assumptions when
  ambiguity could materially change the result; otherwise make a reasonable choice and proceed.
- Read the nearest project instructions, source of truth, enforcement gate, and existing code before
  encoding a rule. A schema shows what can be stored, not what the system permits.
- Seek the root cause before designing machinery. Ask which requirement creates the complexity and
  whether that requirement can be removed or reshaped.
- Make the narrowest coherent change. Preserve unrelated work and avoid opportunistic refactors.
- Do not disguise a workaround as a solution. If a workaround is proportionate, record the cause it
  contains, why the direct fix is unavailable, and what would make the workaround removable.
- Complete the feedback loop: run the checks that observe the changed contract, inspect their output,
  and verify user-visible work through the real interface when practical.
- Report the outcome, evidence, assumptions, and deliberately deferred concerns. Do not claim success
  from code inspection alone when executable verification is available.
- Review — including self-review — with a fault-finding frame: hunt for what is wrong rather than
  grading overall quality. Review by first seeking evidence that would disprove correctness, then assess the overall design. Do not let an encouraging global assessment excuse a concrete defect.

### Design vocabulary

Use these names as precise shorthand:

| Name                              | Meaning here                                                              |
| --------------------------------- | ------------------------------------------------------------------------- |
| Hunt & Thomas, DRY                | one authority for each piece of knowledge, not textual deduplication      |
| Parnas, information hiding        | a module hides a consequential design decision                            |
| Meyer, Single Choice              | decide a distinction once where it first becomes knowable                 |
| Ousterhout, deep modules          | substantial behavior behind a small interface                             |
| Feathers, seam                    | a place where behavior can be altered without editing that place          |
| Alexis King, parse-don't-validate | preserve evidence in a refined value instead of returning ambiguous input |

### Design defaults

- **One authority, explicit derivations.** Multiple representations are legitimate when their source,
  synchronization, staleness, ownership, and rebuild semantics are explicit. A cache or projection
  must not become an accidental second authority. (Hunt & Thomas.)
- **Hide decisions, not files.** A module may be a function, class, package, or tier-spanning slice.
  Its interface includes everything callers must know: types, invariants, ordering, errors,
  configuration, consistency, and performance characteristics. (Parnas.)
- **Prefer deep modules.** Give callers leverage through a small interface and keep change, knowledge,
  and verification local. Apply the deletion test: if removing a module spreads its decision across
  callers, it was useful; if only import paths change, it was ceremony. (Ousterhout.)
- **Let the interface be the test surface.** If callers or tests routinely reach through it, the
  module may hide the wrong thing. Keep internal seams private.
- **Require evidence for seams.** One adapter is hypothetical; two establish real variation. A facade
  may still earn its place by preventing a consequential vendor or package choice from fanning out,
  but do not prebuild speculative dependency injection. (Feathers.)
- **Home state according to lifetime.** When every way of transporting a fact feels awkward—copying,
  summarizing, or reconstructing it—question whether it lives on the wrong object. Cleanup and
  cancellation belong to the runtime that can still invoke them.
- **Decide distinctions once.** Resolve a choice where it first becomes knowable into a value, type,
  or handler that leaves downstream code blind to it. Repeated branches are the smell, not branches
  themselves. Use exhaustive discrimination when the behavior is a genuinely closed set. (Meyer.)
- **Put rules with the context that can enforce them.** Invariants live with the model whose valid
  states they define; parsing and preconditions live at trust boundaries; contextual policies live
  at the decision point with all required facts; authorization is enforced where protected data is
  accessed or changed.
- **Parse, don't validate.** Convert ambiguous input into values that carry the evidence downstream
  code needs. Do not scatter repeated checks over an unchanged, weakly typed value. (Alexis King.)
- **Abstract shared knowledge, not shared shape.** Similar code may encode different decisions.
  Extract when semantics and ownership are genuinely shared; do not import across peer domains merely
  to avoid duplicating a small type.
- **Names tell the truth.** A name must not misrepresent what a thing is, returns, or does; fix the
  name or the behavior rather than leaving them disagreeing.
- **Prefer cohesion over file or function count.** Keep things that change together together; split
  things that change for different reasons. Use composition when collaborators vary independently,
  and keep the happy path linear when guard clauses improve clarity.
- **Comments preserve information code cannot carry.** Record rationale, protocol and concurrency
  constraints, security assumptions, rejected alternatives, and workaround provenance. Do not
  narrate syntax. Promote normative comments to proportionate enforcement when possible.

### Boundaries and change

When the system crosses process, trust, persistence, or deployment boundaries:

- Derive identity, authorization, tenant, and protected routing facts from trusted context. Client
  claims may narrow a request only when disagreement fails closed.
- Prefer commands that express intent over client-composed aggregate state. The authority reads the
  current state, checks policy, applies the operation atomically, and returns the accepted result.
- Define concurrency and retry semantics explicitly. Choose version guards, transactions,
  commutative operations, serialization, idempotency, duplicate detection, or intentional
  last-writer-wins behavior; accidental last-writer-wins is not a strategy.
- Preserve round-trip tokens exactly. Versions, cursors, timestamps, and idempotency keys must not
  lose precision or change representation across a boundary.
- Evolve contracts without requiring synchronized deployment. Expand before contracting: deploy
  compatible readers and writers, migrate, observe, then remove the old form. Preserve rollback.
- Bound external work with timeouts, cancellation, and retry budgets. Retry only operations whose
  idempotency and load consequences are understood.

### Testing

- Test through the smallest interface that observes the contract: focused tests for pure rules,
  contract or integration tests for adapters, and end-to-end tests for critical user flows.
- Make a failure exist before trusting its fix when practical. Verify a regression test by
  reintroducing the regression, observing the intended failure, then restoring the fix.
- Use examples to explain cases and property-based tests (fast-check, QuickCheck, Hypothesis, or
  equivalent) for universal claims. Keep generators representative and total over domain variants;
  preserve minimized failures as regression examples when useful.
- Use mutation testing selectively to measure whether tests detect plausible faults, especially
  around pure, high-value rules. Surviving non-equivalent mutants expose unobserved behavior or weak
  assertions; mutation score is a diagnostic, not a target.
- Prove sophisticated tests, generators, and architecture gates can go red with a deliberate
  negative control before trusting them.
- For eventual positive state, poll the authority. For absence, define the event or observation
  window across which absence matters; neither an instant snapshot nor generic polling proves every
  negative claim.
- Isolate parallel tests by construction with unique data and tracked cleanup. A test double must
  preserve the ordering, failure, and consistency behavior relevant to the contract.
- Verification is proportional to risk, not convenience. Security boundaries, migrations,
  concurrency, and irreversible operations warrant stronger evidence than a local presentation edit.

### Diagnostics

Treat these as triggers to investigate, not automatic verdicts:

- State copied through several layers → does the fact live on an object with the wrong lifetime?
- The same branch repeated → was a distinction decided but never resolved into a useful shape?
- `kind` or `type` checked throughout core logic → is behavior open and capability-based, or is this
  a legitimate exhaustive operation over a closed set?
- A view type named for storage → is presentation re-deciding a persistence concern?
- Empty and absent conflated → does `null` mean "cannot" while `[]` means "can, but empty"?
- A sort feeds equality, hashing, signing, or encoding → is its order total and environment-independent?
- "It is type-only" excuses a dependency violation → what assumptions still cross the seam?
- A wrapper only delegates → what decision, policy, stability, or observability does it provide?
- A cleanup is locally tidier but conceptually awkward → which invariant did the old shape preserve?
- A test or gate has always been green → has a negative control shown it detects the claimed failure?

### Proportion

- Every mechanism has a cost. Right-size structure and enforcement to risk, repetition, and the cost
  of failure; a large codebase does not justify ceremony and a small one does not excuse insecurity.
- Model only what somebody reads, writes, computes, audits, or references. Before adding a field ask
  who consumes it; before adding a module ask which decision it hides.
- Prefer established language, library, framework, and design-system primitives when their contracts
  fit. Adapt cosmetic differences; replace them when behavior or ownership truly differs.
- When borrowing an architecture or pattern, adopt its modeling discipline, not its machinery. Each
  borrowed element earns its place by present need, not fidelity to the source's context.
- Make reversible decisions quickly and irreversible decisions deliberately. Preserve options where
  uncertainty is expensive, not where change is already cheap.
- Keep scope tight. Relocate contextual information instead of cramming or silently deleting it, and
  surface any behavior intentionally removed or deferred.
- Enforce a rule with the earliest reliable, proportionate mechanism: type, exhaustive table, static
  gate, test, runtime assertion, monitoring, or prose. These cover different failures; they are not a
  universal ladder.

### Distillation

1. **Keep it simple; don't get clever.**
2. **Give functions and files clear names and purposes.**
3. **Comments carry what code can't: rationale, not narration.**
4. **Resist premature abstraction.**
5. **Favor composition over inheritance.**
6. **Keep the happy path linear; return early.**
7. **Write tests to enable confident refactoring.**
8. **Promote normative comments to enforcement (Design by Contract).**
9. **Decide a distinction once (Meyer's Single Choice Principle, Replace Conditionals with Polymorphism).**
10. **Home state on the object whose lifetime matches it.**

## Project Guidelines

This is a Tauri application that uses TypeScript/React for the frontend.

## Stellaris environment reference

Facts about the local Stellaris installation, recorded so research does not rediscover them.
These are macOS/Steam defaults on the development machine. In the product, equivalent
locations are user-confirmed Discovery Locations; nothing here is a hard-coded product path.

### Paths

| Purpose | Path |
| --- | --- |
| Install root | `~/Library/Application Support/Steam/steamapps/common/Stellaris` |
| Executable | `<install>/stellaris.app/Contents/MacOS/stellaris` |
| Game data root | `~/Documents/Paradox Interactive/Stellaris` |
| Logs | `<data>/logs` |
| Local mods and descriptors | `<data>/mod` |
| Enabled-mod and disabled-DLC list | `<data>/dlc_load.json` |
| Saves | `<data>/save games` |
| Workshop mods | `~/Library/Application Support/Steam/steamapps/workshop/content/281990` |

### Version pinning

`<install>/launcher-settings.json` is authoritative for the installed build: `version`
(marketing name, currently `Pegasus v4.4.6`), `rawVersion`, and `modsCompatibilityVersion`
(currently `4.4`), plus `exePath` and `exeArgs`.

The first line of `logs/game.log` carries `Game Version: <name>`, but that reflects the last
run rather than what is installed — the two disagree whenever logs are stale. Record both and
trust neither alone.

### Which log answers which question

| Log | Contains |
| --- | --- |
| `logs/error.log` | Collision, duplicate, and malformed-content diagnostics, emitted during database init before any game session |
| `logs/game.log` | Output of the script `log = "..."` effect, and the build banner |
| `logs/setup.log` | `Initializing Database: <name>` lines and per-database contents in initialization order |
| `logs/script_documentation/` | `script_docs` console-command dumps of engine effects, triggers, scopes, modifiers, and localizations — engine surface, not script content |

### Diagnostic shapes that name a winner

These are the greppable forms the game uses when two definitions collide. The
`game_singleobjectdatabase` line is the most valuable: it states which definition survived.

```
[technology.cpp:1418]: Duplicate technology: tech_titan_hull_1
[game_singleobjectdatabase.h:162]: Object with key: is_normal_starbase already exists, using the one at  file: common/scripted_triggers/01_aeo_scripted_triggers.txt line: 1
[designer_database.cpp:74]: A ship design named "Arisen" already exists!  file: common/global_ship_designs/biogenesis_ship_designs.txt line: 2881
[triggered_event_description.inl:22]: Duplicate triggered localization string crisis.7516.desc.center at  file: events/!_giga_overwritten_events.txt line: 992
[reader.cpp:209]: Variable name small_trail_W is already taken.  file: gfx/models/ships/special/acot_corvette_entities.asset line: 27
```

### Mod activation

The game executable itself reads `dlc_load.json`, so activation can be scripted without the
Paradox launcher or its `launcher-v2.sqlite` playset database:

```json
{"enabled_mods":["mod/ugc_3720935310.mod"],"disabled_dlcs":["dlc/dlc033_cosmic_storms/dlc033.dlc"]}
```

Order in `enabled_mods` is the load order. Each entry names a `.mod` descriptor under
`<data>/mod`, whose observed fields are `name`, `version`, `supported_version`, `path`
(absolute), `tags`, `picture`, and `remote_file_id`. `replace_path` is the documented
directory-replacement declaration but appears in none of the mods installed here, so its
behavior is unverified locally.

The executable accepts `-continuelastsave`, which loads the save named by
`<data>/continue_game.json` without menu navigation. A loaded game starts paused, so
`on_action` pulses do not fire until time advances; `on_game_start` fires only for a new game.

### Save format

A `.sav` is a zip containing `gamestate` and `meta`, both Clausewitz text. `gamestate` is
large — roughly 50 MB uncompressed for a mid-game save — so extract and project it rather
than committing it.

Under `country.<id>.tech_status`:

- `alternatives` — the current draw pool, grouped by `physics` / `society` / `engineering`.
- `potential` — the engine-computed set of currently drawable technologies with a per-tech
  value, e.g. `"tech_cruisers"="5"`. This is exported engine state and is the most direct
  observation available for effective technology eligibility.

### Content layout notes

`<install>/checksum_manifest.txt` declares the game's **checksum** scope, which is narrower
than its content-load scope and must not be mistaken for a file-enumeration rule:

```
common/**.txt   common/**.shader   common/**.csv
events/**.txt   map/**.shader      map/**.txt
```

`common/scripted_variables/00_scripted_variables.txt` opens with the vanilla claim that
global `@` variables "can be overridden in each normal script file" — a documented statement
to verify against the game, not to inherit as resolver policy.

### Installed DLC

29 directories under `<install>/dlc`, `dlc002_arachnoid` through `dlc039_stargazer`. The
current `dlc_load.json` disables `dlc033_cosmic_storms`.

### Local tooling

`python3` 3.13.5 at `/opt/homebrew/bin/python3`; `node` v24.15.0.