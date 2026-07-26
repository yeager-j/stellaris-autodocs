# Resolver evaluation

Status: Planned. Every unresolved matrix cell blocks acceptance of the Resolution Profile for that content type.

## Decision

Establish the exact Stellaris semantics required to resolve one Target Mod against the installed base game and DLC. The spike must replace assumptions with reproducible game-oracle evidence and produce a versioned Resolution Profile consumed by `analysis`.

This spike does not implement full Playset composition. It proves the two-layer subset the MVP already claims.

## Reproducible oracle record

Each oracle run records:

- Stellaris marketing version, internal build identifier, and executable checksum.
- Operating system and architecture.
- Installed DLC identifiers, checksums where available, and observed ordering.
- Complete fixture tree committed to this repository.
- Canonical SHA-256 checksum for every fixture file.
- Launcher configuration and exact activation scope.
- Observation steps and commands.
- Raw logs, screenshots, exported state, or other captured evidence.
- Expected effective definitions field by field.
- Expected provenance for contributed, inherited, defaulted, duplicate, and shadowed facts.

An observation must be repeatable from the fixture and instructions without relying on the researcher's memory. UI inspection alone is insufficient when a console command, scripted log, controlled effect, or exported state can expose the effective value more directly.

## Resolution matrix

`Pending` is a deliberate blocker. No registry falls back to a generic last-wins or merge policy.

| Registry or concern | Definition key or unit | Path and directory behavior | Duplicate policy within one layer | Cross-layer collision policy | Field, default, and ordering policy | Required provenance | Oracle status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Exact file-path collision | Normalized logical file path | Pending | Not applicable | Pending | Establish whether the losing file contributes anything | Selected and shadowed file | Pending |
| Directory replacement | Declared replacement directory and descendants | Pending | Not applicable | Pending | Establish interaction with exact-path shadowing | Replacing declaration and every excluded source | Pending |
| Technologies | Technology script identifier | Pending | Pending | Pending | Whole replacement versus inheritance; omitted fields including `potential`; repeated values | Every contributed, inherited, defaulted, duplicate, and shadowed field | Pending; omitted-`potential` case required |
| Megastructures | Megastructure script identifier | Pending | Pending | Pending | Whole replacement versus inheritance; stage and upgrade ordering | Same | Pending |
| Buildings | Building script identifier | Pending | Pending | Pending | Whole replacement versus inheritance; upgrade and prerequisite fields | Same | Pending |
| Ship components | Component template identifier | Pending | Pending | Pending | Whole replacement versus inheritance; repeated modifiers and prerequisites | Same | Pending |
| Events | Event identifier | Pending | Pending | Pending | Namespace behavior; duplicate events; option and immediate ordering | Same, including shadowed event bodies | Pending |
| Scripted triggers | Scripted trigger identifier | Pending | Pending | Pending | Replacement, parameter behavior, and body ordering | Definition and every resolved call site | Pending |
| Scripted effects | Scripted effect identifier | Pending | Pending | Pending | Replacement, parameter behavior, and effect ordering | Definition and every resolved call site | Pending |
| Scripted constants | Constant symbol plus proven scope | Pending | Pending | Pending | Scope, redefinition, forward references, cycles, and exact numeric evaluation | Every definition and resolution edge | Pending |
| Localization | Language plus localization key | Pending | Pending | Pending | File ordering, replacement, and fallback interaction | Winning and shadowed values per language | Pending |
| Sprite definitions | Sprite name | Pending | Pending | Pending | File ordering, texture path replacement, and frame metadata | Winning and shadowed definitions plus resolved asset | Pending |
| Vanilla and DLC layers | Game or DLC source layer | Pending | Pending | Pending | Exact DLC precedence and interaction with base-game definitions | Layer rank and contributing DLC | Pending |
| Target Mod layer | Selected Mod Installation | Pending | Pending | Pending | Exact precedence over Vanilla and DLC for every registry above | Target contribution and every displaced source | Pending |

## Mandatory fixtures

1. A technology defined in Vanilla and redefined by the Target Mod while omitting `potential`.
2. Duplicate definitions of the same key in separate files within one layer.
3. Duplicate definitions in one file.
4. Exact relative-path collisions with and without a directory-replacement declaration.
5. One case for every registry row in the matrix.
6. Constants that are redefined, referenced before definition, cyclic, and used in exact decimal arithmetic.
7. Localization and sprite collisions whose winning values are observable in the same documented entry.
8. At least two installed DLCs that contribute colliding definitions so their effective order is observable.

## Acceptance

The Resolution Profile is accepted only when:

- Every matrix cell has one explicit policy backed by an oracle record.
- The controlled resolver reproduces every oracle result.
- Provenance distinguishes contributed, inherited, defaulted, duplicate, and shadowed facts.
- Reordering fixture creation or filesystem enumeration does not change the result.
- Unsupported or newly introduced content types fail visibly instead of inheriting another row's policy.
- A Stellaris update that changes any result produces a failing oracle test until the profile and analysis version are intentionally revised.

The completed matrix, fixture checksums, captured evidence, and resulting decision remain in this file and are referenced by the parser and revision-bundle corpora.
