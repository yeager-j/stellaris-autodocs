# Resolver evaluation

Status: Pre-implementation evidence collection complete against Stellaris `Pegasus v4.4.6`; Resolution Profile partially resolved.

Every matrix row has been investigated as far as filesystem inspection and the current
game-oracle harness permit. The captured records are sufficient to begin implementing the
resolved policies and to serve as golden expectations for resolver conformance.

The Resolution Profile itself remains partial. Unresolved cells require resolver-backed
investigation and continue to block support for the content types that need them. They do
not make this evidence-collection spike incomplete, authorize a fallback, or block
implementation of unrelated resolved policies.

## Decision

Establish the exact Stellaris semantics required to resolve one Target Mod against the base-game file set. The spike replaces assumptions with reproducible game-oracle evidence and supplies the resolved portion of a versioned Resolution Profile consumed by `analysis`.

This spike does not implement full Playset composition. It investigates the MVP's two-contributor subset: Vanilla Content and one Target Mod. Those contributors are not universally ordered layers.

## Reproducible oracle record

Each oracle run records:

- Stellaris marketing version, internal build identifier, and executable checksum.
- Operating system and architecture.
- Installed DLC identifiers needed to interpret DLC-gated requirements; DLC does not contribute a definition-source order.
- Complete fixture tree committed to this repository.
- Canonical SHA-256 checksum for every fixture file.
- Launcher configuration and exact activation scope.
- Observation steps and commands.
- Raw logs, screenshots, exported state, or other captured evidence.
- Expected effective definitions field by field.
- Expected provenance for contributed, inherited, defaulted, duplicate, and shadowed facts.

An observation must be repeatable from the fixture and instructions without relying on the researcher's memory. UI inspection alone is insufficient when a console command, scripted log, controlled effect, or exported state can expose the effective value more directly.

The environment, activation protocol, and pinned new-game setup are recorded in
[oracle-records/environment.md](./oracle-records/environment.md). Fixtures live in
[fixtures/oracle/](../../fixtures/oracle/) and the harness in
[tools/oracle/](../../tools/oracle/). Every result below was read from a log, an exported save,
or a load-time diagnostic. The one UI observation in the spike is corroborating rather than
primary: research-screen screenshots confirmed the localization values that
`set_empire_name` had already reported, which mattered because that probe technique was
itself unproven.

## Method

Every measurement is a **difference between two runs**, never a single reading. One
observation cannot separate "the mod's definition won" from "vanilla already behaved this
way", and recording the second as the first is exactly the substitution of assumption for
evidence this spike exists to prevent.

Evidence comes from three channels, two of which are independent for most facts:

| Channel | What it shows | Tier |
| --- | --- | --- |
| `logs/error.log` | Which definition the game **registered**, often naming the winner's file and line | Load time; no game session |
| `logs/game.log` via scripted `log` effects | Which definition the game actually **evaluates**, and what value a consumer sees | In game |
| Save `gamestate` `tech_status.alternatives` | Effective technology draw eligibility, as exported engine state | In game |

Registration and evaluation are separate facts. Where both channels exist they agreed on
every case below; a disagreement would have been a finding in itself.

### Controls

- **Canary.** Every run emits facts with known-true values. A run whose canary is missing is
  discarded rather than interpreted, because a fact that did not happen and a logging
  channel that did not deliver are otherwise indistinguishable.
- **Matched pair.** The technology subject and its negative control differ in exactly one
  field. Everything else changed — tier, cost, weight, prerequisites — is changed
  identically in both, so no other field can explain the difference between them.
- **Positive control.** A never-granted new technology, otherwise identical to the subject,
  proves a mod technology can enter the draw at all.
- **Isolation.** A case that can fail catastrophically gets its own mod and its own run.
  Two cases learned this the hard way; see Blast radius below.

### Why the draw pool rather than the exported potential set

`tech_status.potential` is the game's own computed set of drawable technologies and would
be the most direct reading. It is not usable here: the game only serializes it after a
country has run for some years, and it is absent from every fresh save. Confirmed across the
118 local saves available on the development machine: `potential` is absent from every save
dated in the first game years and appears, then grows, in later ones.

`alternatives`, the current draw pool, is always present. Each technology subject therefore
carries a dominating weight so that "drawable" reliably means "drawn", converting a
probabilistic reading into a decisive one.

## Resolution matrix

`Pending` is a deliberate blocker. No registry falls back to a generic last-wins or merge policy.

| Registry or concern | Definition key or unit | Path and directory behavior | Duplicate policy within one semantic stream | Cross-source collision policy | Field, default, and ordering policy | Required provenance | Oracle status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Exact file-path collision | Normalized logical file path | Winning file replaces the losing file entirely | Not applicable | Target Mod file wins over Vanilla Content | Losing file contributes nothing, including keys the winner never mentions | Selected and shadowed file | **Resolved** — `r6-pathcollision` |
| Directory replacement | Declared replacement directory and descendants | `replace_path` excludes every other source's files in that directory; the declaring mod's own files in it still load | Not applicable | Excluded sources contribute nothing | Exclusion is by directory, independent of which keys the replacement defines | Replacing declaration and every excluded source | **Resolved** — `r3-replace-path` |
| Technologies | Technology script identifier | Follows file-path and directory rules above | Last in enumeration order wins | Decided by path order, not by layer: whichever definition the registry's accept/reject rule selects from the one global enumeration. A Target Mod wins only when its filename sorts on the winning side — a late-sorting name (`r1`), and vanilla wins against an early one (`r10`) | Whole-object replacement; an omitted field is absent, not inherited — proven for `potential` | Every contributed, defaulted, duplicate, and shadowed field | **Resolved** — `r0`, `r1`, `r4`; omitted-`potential` case done |
| Megastructures | Megastructure script identifier | Follows file-path and directory rules above | Last in enumeration order wins | Decided by path order, not by layer: whichever definition the registry's accept/reject rule selects from the one global enumeration. A Target Mod wins only when its filename sorts on the winning side | **Inconclusive** — the registry's diagnostics report missing localization and sprites, which vanilla supplies regardless of the definition, so they cannot detect field inheritance | Same | Partial — `r8`; merge test needs a runtime observable |
| Buildings | Building script identifier | Follows file-path and directory rules above | Last in enumeration order wins | Decided by path order, not by layer: whichever definition the registry's accept/reject rule selects from the one global enumeration. A Target Mod wins only when its filename sorts on the winning side | Whole-object replacement; a redefinition omitting `building_sets` is diagnosed exactly as a new key omitting it | Same | **Resolved** — `r8` |
| Ship components | Inner `key` field, **not** the `utility_component_template` block name | Follows file-path and directory rules above | **Undetermined** — the diagnostic reports `key used multiple times` without naming a winner | Undetermined, reported the same way as a same-source duplicate | Whole-object replacement; a redefinition omitting `icon` is diagnosed exactly as a new key omitting it | Same | Partial — `r8`; winner needs a runtime observable |
| Events | Namespace plus number | Same-path replacement also works | **First** registration wins; the later one is rejected | **Overridable by identifier when the mod's file sorts before vanilla's.** A `!!!_`-prefixed file wins (`r10`); a `zz_`-prefixed one loses (`r9`) | Not applicable — the rejected definition is discarded whole | Same, including shadowed event bodies | **Resolved** — `r8`, `r9`, `r10` |
| Scripted triggers | Scripted trigger identifier | Follows file-path and directory rules above | Last in enumeration order wins, within a file and across files | Decided by path order, not by layer: whichever definition the registry's accept/reject rule selects from the one global enumeration. A Target Mod wins only when its filename sorts on the winning side | Whole replacement; the shadowed body never evaluates | Definition and every resolved call site | **Partial** — collision and replacement resolved by `r1`, `r4`; parameter behavior requires resolver-backed investigation |
| Scripted effects | Scripted effect identifier | Follows file-path and directory rules above | Last in enumeration order wins; duplicates do not accumulate | Decided by path order, not by layer: whichever definition the registry's accept/reject rule selects from the one global enumeration. A Target Mod wins only when its filename sorts on the winning side | Whole replacement; only the winning body executes | Definition and every resolved call site | **Partial** — collision and replacement resolved by `r1`, `r4`; parameter behavior requires resolver-backed investigation |
| Scripted constants | Constant symbol, global with file-local override | Follows file-path and directory rules above | **First** in enumeration order wins; later ones are rejected | Pending for Vanilla-versus-Target | A file-local declaration overrides the global for that file; forward references and cycles do not resolve; decimal arithmetic is exact | Every definition and resolution edge | **Partial** — arithmetic and same-source behavior resolved by `r1`, `r4`, `r5`, `r7`; cross-source behavior requires resolver-backed investigation |
| Inline scripts | Normalized path under `common/inline_scripts`, one script per file, extension dropped | Same-path replacement is the ONLY collision mode; there is no declared identifier to collide on | Not applicable — one script per file | A mod file at a vanilla script's path replaces its content entirely | Textual expansion into the consuming definition before it is registered. `$PARAM$` substitution works; inclusion nests and must be expanded recursively. An unresolved reference is diagnosed with consuming file and line, but the definition still registers with the inclusion silently omitted | Every expansion site, its resolved source path, and its parameter bindings | **Resolved** — `r11`, `r12` |
| Localization | Language plus localization key | Exact path collision shadows the whole vanilla file: every key the winning file omits renders as its raw key. Scoped to that file — keys in other vanilla files are untouched | Last loaded wins (LIOS) | One ordered stream over surviving files: Vanilla, then mod files in `enabled_mods` order, then every `replace/` file. A mod beats Vanilla regardless of filename; an earlier mod loses to a later one; `replace/` wins from any position | No fallback language — a missing key renders as the raw key. References resolve against the EFFECTIVE post-collision value and propagate into strings owned by other sources | Winning and shadowed values per language, each reference edge with the source of the value it resolved to, and every key lost to a shadowed file | **Resolved** — `r13`, `r14`, `r15`, `r16` |
| Sprite definitions | Sprite name inside a `spriteTypes` block, across all `interface/*.gfx` | Follows the file-path and directory rules above | Last in enumeration order wins, within a file and across files | Decided by path order, not layer, exactly as script registries: a mod file sorting after vanilla's wins (`r17`), one sorting before it loses (`r18`) | Whole replacement of the named sprite. `sprite_sheet_sprite_type` references resolve to the winning definition, so overriding one sprite changed the resolved texture of 54 vanilla sprites that referenced it | Winning and shadowed definition, the resolved texture path, and every sprite referencing the changed one | **Resolved** — `r17`, `r18` |
| DLC as a definition source | Not applicable — **DLC is not a content layer** | DLC archives ship no script, localisation, interface, or map files at all | Not applicable | Not applicable: there is no DLC content to collide with Vanilla Content | DLC-gated definitions ship inside base-game files and are evaluated through `host_has_dlc`. Installing or removing a DLC does not change which definitions load, only whether their gates pass | The `host_has_dlc` condition on a gated definition, recorded as a requirement rather than as a source layer | **Resolved** — filesystem survey, no game run required |
| Target Mod contributor | Selected Mod Installation | Replaces Vanilla Content on an identical logical path | Not applicable | **There is no layer precedence for distinct paths.** The Target Mod's definitions are enumerated in one global path order alongside Vanilla Content | Applies per registry; never generalized | Target contribution and every displaced source | **Resolved** — `r10` |

## Findings

### Duplicate policy is not uniform across registries

The single most important result. Within one game build, two registries resolve duplicates
in **opposite** directions:

| Registry | Winner | Diagnostic |
| --- | --- | --- |
| Scripted triggers, scripted effects, technologies, buildings, megastructures | **Last** | `Object with key: X already exists, using the one at file: … line: N` — naming the later definition as the survivor |
| Scripted constants | **First** | `Variable name X is already taken. file: … line: N` — naming the later definition as *rejected* |
| Events | **First** | `an event with id [X] already exists! file: … line: N` — naming the later definition as *rejected* |

All three messages point at the second definition; they do not mean the same thing.
Reading any of them as a generic "duplicate" would produce a resolver that is wrong for one
group or the other — and wrong in the worst direction, since it would silently attribute the
wrong body to the wrong source.

This is the concrete justification for the design's refusal to let a missing policy row
fall back to a generic merge (`docs/technical-design.md:279`). A uniform last-wins resolver
would be silently wrong for every scripted constant in every mod.

Evidence, `r1-target`:

```
[game_singleobjectdatabase.h:170]: Object with key: oracle_dup_same_file already exists,
    using the one at  file: common/scripted_triggers/zz_oracle_triggers_a.txt line: 26
[reader.cpp:209]: Variable name oracle_const_same_file is already taken.
    file: common/scripted_variables/zz_oracle_variables_a.txt line: 12
```

Line 26 is the second of two trigger definitions; line 12 is the second of two constant
definitions. Runtime evaluation agreed independently: `trigger_dup|same_file|B_last` and
`const|same_file|1_first`.

### The winner is decided by enumeration order, not by content

`r4-reordered` ships byte-identical fixture content with the `_a` and `_b` filenames
swapped, so the definition that sorted last now sorts first. The winners flipped, and they
flipped in the direction a position rule predicts.

The probe's labels are tied to content, not position, which is what makes the result
readable:

| Run | `…_a.txt` holds | `…_b.txt` holds | Winning content | Winning position |
| --- | --- | --- | --- | --- |
| `r1-target` | trigger `always = yes` | trigger `always = no` | `no` | last file |
| `r4-reordered` | trigger `always = no` | trigger `always = yes` | `yes` | **last file** |
| `r1-target` | constant `10` | constant `20` | `10` | first file |
| `r4-reordered` | constant `20` | constant `10` | `20` | **first file** |

Swapping the names swapped which content won while the winning position held. `error.log`
agrees independently, naming `zz_oracle_triggers_b.txt` as the surviving trigger in both
runs and `zz_oracle_variables_b.txt` as the rejected constant in both.

Within-file results did not move — `same_file|B_last` and `const|same_file|1_first` in both
runs — because the swap changed file names, not the order of definitions inside a file.

So the policies are positional:

- Single-object registries — scripted triggers, scripted effects, technologies: **last** in
  enumeration order wins, whether that means later in a file or a later file.
- Scripted constants: **first** in enumeration order wins.

The resolver must therefore enumerate by normalized logical-path bytes and definition
ordinal to reproduce these results, which is the ordering the design already specifies for
canonical encoding (`docs/technical-design.md:313`). Enumerating in raw filesystem order
would produce correct-looking results that vary by platform and directory state.

### Technology redefinition is whole-object replacement

The technical design requires this be validated rather than assumed. It holds and is now
evidence.

The subject and the negative control are identical in every field the fixture changed —
tier 0, cost 1000, weight 1000000, no prerequisites — and differ only in whether vanilla's
`potential` block was retained:

```
DRAWN   tech_oracle_draw_control          positive control - must be drawn
DRAWN   tech_adaptive_combat_algorithms   SUBJECT - potential omitted
absent  tech_neural_implants              negative control - must be absent
```

The retained `potential` still blocked; the omitted one did not. An omitted field is
genuinely absent rather than inherited from the shadowed definition.

This result is scoped to technologies. Buildings and ship components have their own
whole-object evidence, megastructure field behavior remains inconclusive, and events use a
separate first-registration rule rather than field inheritance.

### An exact path collision replaces the whole file

The mod's `common/technology/00_astral_planes_tech.txt` defines one sentinel and neither of
the two technologies vanilla defines at that path. Both vanilla technologies vanished, and
61 references across vanilla events, deposits, situations, anomalies, and starbase modules
failed to resolve:

```
[parser_deferred_database_objects.cpp:84]: Failed to deferred read key reference
    tech_astral_harvesting from database  file: common/anomalies/101_anomaly_categories_astral_planes.txt line: 33
[technology_level.cpp:41]: Invalid technology being referenced: "tech_rift_sphere"
```

Merge-by-key is ruled out: the losing file contributes nothing, not even keys the winner
never mentions. Startup does not survive it when vanilla hard-references the lost keys.

### `replace_path` excludes other sources, keeps your own

`replace_path="common/technology"` produced 7,708 error lines: 6,886 unresolved deferred
key references and 591 `Invalid technology being referenced`, spanning the whole vanilla
technology tree.

The declaring mod's own files in the replaced directory still loaded — all three of its new
technologies registered. So the rule is *exclude every other source's files in this
directory*, not *this directory now contains only what I ship and nothing resolves*.

### Scripted constants

| Case | Result | Channel |
| --- | --- | --- |
| Redefined within one file | First wins | Both |
| Redefined across two files in one layer | First file wins | Both |
| Declared in the consuming script file | File-local declaration overrides the global for that file | Runtime |
| Forward reference | Does not resolve | Load time |
| Cycle | Does not resolve | Load time |
| `0.1 + 0.2` compared against `0.3` | Exactly equal | Runtime |
| Vanilla `@tier5cost3` read from a mod file | Resolves | Runtime |

The exact-decimal result supports the design's requirement that binary floating point never
participate in source equality or displayed Base Values (`docs/technical-design.md:324`):
the game itself compares these values exactly.

**Unresolved references fail silently and destructively.** An unresolved `@name` is not
rejected. The parser reinterprets the field as a script-value block and swallows the
following lines:

```
[meantimetohappen.cpp:853]: unknown command 'tier' for MTTH/script value
    in file common/technology/zz_oracle_fwd_consumer.txt line : 16
[technology.cpp:724]: Technology "tech_oracle_fwd_consumer" is missing tier
```

The definition survives in a corrupted state rather than being rejected. The resolver must
detect unresolved `@` references itself; the game's diagnostics do not reliably identify
them, and a definition that merely looks complete may not be.

A separate run with the same constants declared but **never consumed** produced no
diagnostic at all. The game validates a scripted constant where it is read, not where it is
declared, so an unused broken constant is invisible at load time.

### Registry survey: what a shared diagnostic does and does not prove

`r8-registries` tested megastructures, buildings, ship components, and events with a
comparative design that does not require knowing which fields the game treats as mandatory:
one **new** key and one **redefinition** of a vanilla key share an identical minimal body.
The new key cannot inherit anything, so whatever it is diagnosed for is the baseline that
inheritance would have suppressed.

**Buildings and ship components are whole-object replacement**, confirming the same rule
found for technologies:

```
[building_type.cpp:761]: Building 'oracle_building_new' is lacking the building_sets definition.
[building_type.cpp:761]: Building 'building_embassy' is lacking the building_sets definition.
[component.cpp:843]: component ORACLE_COMPONENT_NEW has no icon.
[component.cpp:843]: component SMALL_ARMOR_1 has no icon.
```

Vanilla's `building_embassy` has `building_sets` and vanilla's `SMALL_ARMOR_1` has an
`icon`. Both redefinitions are diagnosed identically to a key that never had them, so
nothing was inherited.

**The megastructure merge test is inconclusive, and its silence must not be read as
inheritance.** The registry's diagnostics report missing *localization keys* and *outliner
sprites* — external resources that vanilla supplies for `think_tank_0` whatever the
definition says. The redefinition was therefore quiet for a reason unrelated to
inheritance. The control was diagnosed only because a brand-new key has no vanilla
localization. This observable cannot discriminate, so the cell stays open.

**Ship components are keyed by an inner field.** The block identifier is a type shared by
thousands of definitions:

```
utility_component_template = { key = "SMALL_ARMOR_1" ... }
```

A collision rule that assumed the block name is the key would collapse every utility
component in the game into one entry. Their duplicate diagnostic is also different in kind
— `Component template key used multiple times: ORACLE_COMPONENT_DUP` names no winner — so
the winner for that registry is undetermined.

### There is no layer precedence. There is one path order.

The most consequential result in this spike, and a correction to cells earlier runs
appeared to settle.

`r10-loadorder` ships one mod whose files are named `!!!_…`, sorting ahead of almost every
vanilla file, and applies that name to both a first-wins registry and a last-wins one. The
two enumeration models predict opposite pairs, so the outcome identifies the model:

| Subject | Registry rule | Mod file | Result |
| --- | --- | --- | --- |
| `story.5` | first wins | `events/!!!_oracle_story_override.txt` | **Mod won.** Vanilla's registration is the one rejected: `an event with id [story.5] already exists! file: events/story_events.txt line: 1212` |
| `tech_adaptive_combat_algorithms` | last wins | `common/technology/!!!_oracle_tech.txt` | **Vanilla won**: `using the one at file: common/technology/00_synthetic_dawn_tech.txt line: 347`, and the technology is absent from the draw |

Under a layer model the mod would register after Vanilla in both cases, winning the
technology and losing the event. The exact opposite happened. Enumeration is therefore **one
global order over both definition sources, keyed by normalized path**, with no separate rank
for Vanilla Content or the Target Mod.

The same mod content flips outcome purely on filename:

| Run | Mod filename | Events | Technologies |
| --- | --- | --- | --- |
| `r1` / `r9` | `zz_…` (sorts late) | mod **loses** | mod **wins** |
| `r10` | `!!!_…` (sorts early) | mod **wins** | mod **loses** |

**"The Target Mod wins" is not a rule.** Where earlier runs reported it, that was an artifact
of fixtures named `zz_…`, which happened to sort after vanilla. Those cells are corrected
below. This is exactly the failure mode the differential method is meant to catch, and it
was caught only because a practitioner said the conclusion contradicted real modding
practice.

Two mechanisms compose:

1. **Same logical path** — one file replaces the other outright, and here the source layer
   does decide: the Target Mod's file replaces Vanilla's (`r6`, `r3`). Identical paths
   cannot be ordered against each other, so layer is the tiebreak.
2. **Distinct paths** — every surviving file is enumerated in one path order regardless of
   source, and each registry either accepts or rejects a repeat registration.

Cross-source precedence is a *consequence* of those two, never an independent rule. It is
why the matrix records path behavior and duplicate behavior separately, and why no row
states a bare precedence.

This finding required a design correction. Stable Source Snapshot inventory remains
independently ordered for fingerprints and parsing, while `analysis` now constructs the
semantic stream per content type. Source origin remains provenance but never substitutes
for script or sprite resolution order.

### The remaining rule: accept or reject a repeat

Every registry registers definitions in the one enumeration order established above, and
each either accepts or rejects a later registration for a key it already holds. That single
choice, combined with where a mod's filename sorts, explains every collision result in this
spike — and explains the naming conventions visible in the installed mod corpus:

| Behavior on a repeat registration | Registries | To win, a mod's file must sort |
| --- | --- | --- |
| Later **replaces** earlier | Technologies, buildings, megastructures, scripted triggers, scripted effects | **after** the file it overrides (`zz_…`) |
| Later is **rejected** | Events, scripted constants | **before** the file it overrides (`!!!_…`, `000_…`) |

Combined with the path-order finding above, the complete model is:

```
1. Resolve same-path file collisions   (Target Mod's file replaces Vanilla's)
2. Enumerate every surviving file in one global normalized-path order
3. Within a file, take definitions in source order
4. On a repeat registration, apply the registry's rule: replace, or reject
```

Nothing in that model mentions a layer except step 1. A resolver that ranks sources before
paths will get steps 2 and 4 wrong in opposite directions for the two registry groups.

**This yields a prediction the spike has not tested.** Scripted constants reject later
registrations, so a Target Mod redefining a vanilla `@` constant should win only from an
early-sorting file, exactly as events do. The constants cross-source cell stays `Pending`:
deriving a cell from a neighbouring result is what this spike exists to avoid. It is
recorded as the next thing to measure, not as policy.

### Events override by identifier only from an early-sorting file

`r8` suggested this from the wording of a log message. `r9` confirmed it by firing the
identifiers from the console and observing which body ran.

**Within one layer, the first registration wins.** `oracle_rt.1` is defined twice; the first
body logged:

```
ORACLE|event_dup_runtime|winner|first_1111
[eventmanager.cpp:427]: an event with id [oracle_rt.1] already exists!
    file: events/zz_oracle_runtime_events.txt line: 45
```

Line 45 is the second definition, and it is the one reported as rejected.

**Across layers, Vanilla wins.** A mod redeclaring `story.5` did not replace it. Firing the
identifier produced the vanilla "Contact Report: Remnants" popup, the game recorded a human
option selection against the vanilla event, and the mod body never logged:

```
[eventcommands.cpp:88]: Event story.5 added info about event selection. selectedOption 6, human 1
[eventmanager.cpp:427]: an event with id [story.5] already exists!
    file: events/zz_oracle_story_override.txt line: 46
```

The rejected registration is the mod's. Zero `event_override_runtime` lines appear in the
log while the console canary and both other probes logged normally, so the absence is a
real negative rather than a dead channel.

**This was initially over-read.** `r9` showed only that a `zz_…`-named file loses, and the
first write-up of this section concluded that events were not overridable by identifier at
all. A practitioner objection — that overriding events is known to be possible — prompted
`r10`, which shows a mod *does* replace a vanilla event by redeclaring its identifier,
provided its file sorts before vanilla's. Both observations are consistent; the first was
simply not general.

The installed mod corpus shows the convention in the filenames themselves:
`!_giga_overwritten_events.txt`, `!!_acot_exterminatus_events_override.txt`,
`!!!_AR_overwrite.txt`, `000_spawn_override.txt`. Those prefixes are not decoration; in a
first-wins registry they are the override mechanism.

This matters directly to the technology vertical slice. Technology documentation must
inspect the events that grant technologies (`docs/decision-log.md` D-004), so the resolver
must decide event collisions by enumeration position and the first-wins rule. Applying the
technology rule to events would pick the wrong body in either direction depending on
filenames, and silently document unlock routes that do not exist.

#### Subject selection

The first attempt used `utopia.1`, whose trigger requires a neighbouring empire to be
building a ring world. That condition is false in a single-empire probe game, so vanilla
winning and vanilla's trigger suppressing the event would both have produced silence, and
the measurement could not have distinguished them. `story.5` was chosen instead because its
trigger — `is_country_type = default` and the absence of four story flags — is satisfied by
a fresh United Nations of Earth start.

The block type was also matched to vanilla's `event = {`; the `r8` fixture had declared the
override as a `country_event`, which varied the layer and the declaration form at once.

The result that most justified running this rather than accepting the expectation.

```
[eventmanager.cpp:427]: an event with id [oracle_events.1] already exists!
    file: events/zz_oracle_events.txt line: 25
[eventmanager.cpp:427]: an event with id [utopia.1] already exists!
    file: events/zz_oracle_event_override.txt line: 17
```

Both messages name the **second** registration, and in the cross-source case that second
registration is the mod's. The wording matches the scripted-constant message
(`already taken`, where the later definition is rejected) rather than the trigger message
(`using the one at`, which names the survivor). If that reading holds, a mod cannot replace
a vanilla event by redefining its identifier — only by winning the file-path collision and
replacing the whole file.

That is consistent with established modding practice, where vanilla events are overridden
by shipping a replacement file rather than by redeclaring an id in a new file. It is also
consistent with a practitioner report that overriding an event replaces it entirely: a
file-level override *is* whole replacement, and this result says the replacement happens at
the file level, not the key level.

**This is registration evidence only and is recorded as unconfirmed.** Wording is not
behavior, and the whole point of the two-channel design is that the registry that logs a
collision is not always the one that decides evaluation. Confirming it requires firing both
identifiers in a running game and observing which body executes.

### Localization is layer-ordered, where script content is path-ordered

The second failure of an assumed uniformity, and this time the assumption came from the
community wiki rather than from me.

The [localisation modding wiki page](https://stellaris.paradoxwikis.com/Localisation_modding)
states that duplicate keys resolve **LIOS**, "Last in, only served", and that the `replace/`
subfolder works because its files "load after all other localisation files". Read together
those describe a single ordered stream, which predicts that a mod file sorting *before*
vanilla's loses.

`r13-loc-methods` tested that with two files of identical intent on opposite sides of
vanilla's `technology_l_english.yml`, the same discriminator that overturned the layer model
for scripts in `r10`:

| Key | Override method | LIOS predicts | Observed |
| --- | --- | --- | --- |
| `tech_physics_1` | plain file, `zz_…`, sorts after vanilla | mod wins | `ORACLE_PLAIN_FILE` |
| `tech_basic_science_lab_1` | plain file, `00_…`, sorts **before** vanilla | **vanilla wins** | **`ORACLE_EARLY_FILE`** |
| `tech_society_1` | `replace/` subfolder | mod wins | `ORACLE_REPLACE_FOLDER` |
| `tech_engineering_1` | none | `Nanomechanics` | `Nanomechanics` |

The early-sorting file won. **A mod's localisation file beats Vanilla regardless of its
filename**, which is the opposite of what `r10` established for script registries, where a
mod file named `!!!_…` registered before Vanilla and lost a last-wins registry because of
it.

So the enumeration model is not uniform across content types:

| Content | Ordering | A mod file sorting before Vanilla's | Established by |
| --- | --- | --- | --- |
| Script registries (`common/`, `events/`) | one global path order, no layer | registers first, and loses a last-wins registry | `r10` |
| Sprites (`interface/*.gfx`) | one global path order, no layer | registers first, and loses | `r17`, `r18` |
| Localisation (`localisation/`) | layer order — every mod file after every Vanilla file | still wins | `r13`, `r15` |

Localisation is the sole exception measured. That is a narrower and more defensible claim
than "every content type differs", and it is why the ordering model is recorded per content
type rather than once.

A resolver applying one rule to both is wrong for one of them. `replace/` is also not a
distinct mechanism against Vanilla: an ordinary mod file won just as decisively. Per the
wiki it merely loads last, which matters mod-against-mod rather than mod-against-Vanilla.

**What is established and what is not.** The measurement shows *mod beats Vanilla
regardless of filename*. It does not isolate whether that is true layer ranking or a
by-product of sorting on absolute paths, since mod files live under a different root than
the Steam install. Distinguishing the two would need a mod whose absolute path sorts ahead
of the install directory. The practical rule for the resolver is identical either way, so
the ambiguity is recorded rather than resolved.

The four readings also validated the instrument. `set_empire_name` accepts a localization
key, so setting the empire name and logging `[This.GetName]` renders an effective localized
value into `game.log` — a technique invented for this run and therefore unproven. Four
distinct values proved the effect resolves immediately rather than being deferred, and
research-screen screenshots independently showed the same four strings, with neighbouring
tooltip keys still rendering their vanilla text.

A rejected alternative is worth recording. The probe's existing
`ORACLE|canary|country|United Nations of Earth` line looks like a localized value, and an
early plan was to use it as the localization observable. It was rejected because the path
from that string back to a localization key is not established: `EMPIRE_DESIGN_humans1` is
a real key with that text, but an empire's name is also editable at creation and may be
stored as composed text rather than re-resolved per read. Whether an override would move it
was never measured, and a probe whose resolution path is unverified cannot support a
negative result. `set_empire_name` on an explicit key was used instead precisely because it
takes a key by contract.

### DLC is not a content layer

The matrix asked for "exact DLC precedence and interaction with base-game definitions", and
mandatory fixture 8 asked for two DLCs contributing colliding definitions. Both rest on a
premise that is false: **no DLC ships script content at all.**

Every one of the 30 installed DLC archives contains zero files under `common/`, `events/`,
`localisation/`, `interface/`, or `map/`. They hold music, sound, and art; several — including
`dlc002_arachnoid`, `dlc029_firstcontact`, and `dlc039_stargazer` — are empty archives.

```bash
# Reproducible without launching the game.
python3 - <<'EOF'
import zipfile, pathlib
root = pathlib.Path("dlc")           # relative to the Stellaris install
for d in sorted(root.iterdir()):
    for z in d.glob("*.zip"):
        names = zipfile.ZipFile(z).namelist()
        script = [n for n in names
                  if n.startswith(("common/", "events/", "localisation/", "interface/", "map/"))]
        print(f"{d.name:26s} {len(names):5d} entries  {len(script)} script files")
EOF
```

DLC script content ships **inside the base game** and is gated at evaluation time:

```
common/technology/00_apocalypse_tech.txt   # base game, not the Apocalypse archive
    tech_colossus = { potential = { host_has_dlc = "Apocalypse" } ... }
```

69 base-game files carry `host_has_dlc`, covering 26 distinct DLC names.

**Consequences for the resolver.** The loaded definition set is DLC-independent: installing
or removing a DLC changes only whether a gate evaluates true, never which definitions exist.
So there is no DLC layer to rank, no DLC precedence to establish, and no ordering interaction
to measure. `host_has_dlc` is an ordinary requirement condition, and belongs in documentation
as a requirement — "requires the Apocalypse DLC" — rather than in the Resolution Profile as
a source layer.

This also sharpened the vocabulary. `CONTEXT.md` previously described Vanilla Content as
base-game and locally available DLC content, which implied a separate contributing source.
The glossary and technical design now define Vanilla Content as the base-game file set and
treat DLC availability as a requirement fact instead.

The MVP's two-contributor claim survives intact and gets simpler: Vanilla Content is the
base-game file set, and the Target Mod is the only other definition contributor.

### Sprites, and an observable that answered the wrong question

Sprites are a visual registry, so the naive observation is to look at the icon. The
substitute was to point the two candidate definitions at different texture paths, only one
of which exists, and let the missing-texture diagnostic name the resolved definition.

That observable is **necessary but not sufficient**, and reading it as sufficient produced a
wrong conclusion that survived until a signal nobody designed contradicted it.

```
[spritetype.cpp:317]: Error initialising texture:
    gfx/interface/icons/oracle_missing_vanilla.dds for spritetype GFX_alerticons
```

Exactly one such line appears **whether the mod's definition wins or loses**, because it
reports that the game read the definition and tried to initialise its texture — not that the
definition survived. The first reading of `r18` took it as a win and recorded sprites as
layer-ordered, which is the opposite of the truth.

The discriminator is vanilla's dependents. `interface/alerts.gfx` defines about 54 sprites
carrying `sprite_sheet_sprite_type = "GFX_alerticons"`, and those resolve through whichever
definition won:

| Run | Mod file sorts | `GFX_alerticons` | 54 dependents | Winner |
| --- | --- | --- | --- | --- |
| `r17-sprites` | after `alerts.gfx` | 1 | **54** | mod |
| `r18-sprites-early` | before `alerts.gfx` | 1 | **0** | vanilla |

So sprites are **path-ordered, last-wins**, the same as script registries. Localisation
remains the only content type measured that orders by layer instead.

Within a mod, `r17` also showed last-wins both inside one file and across two files, matching
every other single-object registry.

**The dependent signal was luck, not design.** It existed only because `r17`'s subject
happened to be a sprite sheet with referents. Had the chosen subject been a standalone
sprite, both runs would have produced one identical line and the wrong conclusion would have
been recorded with a clean-looking record behind it. The lesson for the remaining rows is
that an observable must be shown to distinguish *both* outcomes before its silence or its
noise is treated as an answer — the same discipline the canary provides for the logging
channel.

### Sprite references propagate like localization references

Overriding one sprite changed the resolved texture of 54 vanilla sprites that named it
through `sprite_sheet_sprite_type`. This is the structural twin of the localization
reference result: a definition's effective content can depend on a key its own source never
mentions.

Provenance for this row therefore needs the reference edge and the source of the resolved
value, not merely the winning definition — otherwise a documented icon could be attributed
to the mod that defined the referring sprite rather than the one that supplied the texture.

### The localization stream, and what `replace/` actually is

`r13` compared a mod against Vanilla only, which left two questions it structurally could
not answer: whether ordering inside the mod layer follows load order, and whether `replace/`
has real priority or merely rode the mod layer. With Vanilla as the sole opponent both
hypotheses predict the same reading.

`r15-loc-modvmod` answers both by putting a **later-loading mod** on the other side. The
fixture is loaded *before* Gigastructural Engineering, so the two override methods make
opposite predictions:

| Key | Our override | Result | Reading |
| --- | --- | --- | --- |
| `name_alderson` | plain file, our mod loads first | `Alderson Disk` | **Giga won** — LIOS by load order inside the mod layer |
| `giga_start_screen_alderson` | `replace/`, our mod loads first | `ORACLE_MODVMOD_REPLACE` | **`replace/` won anyway** — genuine priority |
| `tech_engineering_1` | none | `Nanomechanics` | Vanilla cross-check |

All three localization results then collapse into one ordered stream:

```
Vanilla files
  -> mod files, in enabled_mods order
    -> every replace/ file, from any mod
       LIOS throughout: the last loaded value wins
```

That explains a mod beating Vanilla regardless of filename (`r13`), an earlier mod losing to
a later one (`r15`), and `replace/` winning from any position (`r15`). Filename ordering,
which decides script registries, plays no part.

### A same-named localisation file destroys the vanilla file's other keys

Ordering and path collision are independent questions, and `r13`–`r16` only answered
ordering. A same-named file could plausibly have merged its keys into the stream rather than
displacing vanilla's file wholesale.

It does not. `r14-loc-samepath` ships `localisation/english/technology_l_english.yml`
defining one key, and three other vanilla keys from that same file render as their own names:

```
ORACLE|loc|tech_physics_1|ORACLE_SAME_PATH
ORACLE|loc|tech_society_1|tech_society_1
ORACLE|loc|tech_basic_science_lab_1|tech_basic_science_lab_1
ORACLE|loc|tech_engineering_1|tech_engineering_1
```

The damage is scoped exactly to the colliding file. During the run `tech_bio_reactor` still
rendered as "Bio-Reactor" because vanilla defines it in `main_2_l_english.yml`, a file
nothing collided with — a natural control that separates "this file was shadowed" from
"localization broke".

So localisation obeys **both** mechanisms, and they compose in order:

1. Exact path collision removes losing files entirely — the `r6` rule, unchanged.
2. Whatever files survive merge per key in the ordered stream above.

**This is a documentation hazard with a specific shape.** The wiki notes there is no
fallback language, and the readings confirm it: a lost key renders as its raw identifier. A
mod that ships a same-named localisation file to rename one technology silently blanks every
other name in that vanilla file. Player Documentation would then show hundreds of entries
titled `tech_society_1`.

The app must not present a raw-key rendering as a name. A key that resolves to itself is
evidence of a shadowed file rather than a legitimate value, and the difference between a
deliberate rename and hundreds of casualties is exactly what an Analysis Issue should carry.
That is why the provenance column for this row demands the keys lost to a shadowed file, not
only the winning values.

### Localization references resolve against effective values

Giga defines `origin_alderson` as the literal string `"$name_alderson$"` — a reference
rather than a value. Whether such a reference sees the *effective* value of its target or
the value its own mod wrote determines whether the localization module can resolve its
reference graph once or must resolve it per source.

`r15` could not answer it. The reference's target sat behind an override that lost, so the
effective value was Giga's either way and both hypotheses predicted the same string. The
subject was placed behind the one arrangement that cannot discriminate.

`r16-loc-reference` fixes that by overriding `name_alderson` from `replace/`, which `r15`
proved wins from any position, and leaving `origin_alderson` untouched:

```
ORACLE|loc|name_alderson|ORACLE_REFERENCE_TARGET
ORACLE|loc|origin_alderson_reference|ORACLE_REFERENCE_TARGET
```

**References resolve against the effective, post-collision value.** The rendered origin
description — a string no fixture ever touched — became "living in the vast expanse of an
ORACLE_REFERENCE_TARGET built by unknown precursors", so an override propagates through
references into text owned by another source entirely.

For `localization` this means the reference graph is resolved once, after collision
resolution, against the winning value of each key. Provenance must record more than the
winning value: a displayed string can depend on a key its own source never mentions, so each
reference edge needs the source of the value it resolved to.

### Inline scripts are a fourth substitution mechanism, and they fail quietly

Inline scripts are textual inclusion: a file under `common/inline_scripts` is expanded into
the consuming definition as if written there, optionally with `$PARAM$` substitution. They
are keyed by **file path** with one script per file, so they have no declared identifier and
cannot participate in the accept/reject registration rule. Their only collision mode is the
same-path replacement of `r6`.

They matter more than their obscurity suggests. Vanilla technologies use them heavily — 31
uses in `00_ancient_relics_tech.txt` alone — and specifically inside `weight_modifier`,
which is the source of Base Draw Weight and every conditional Weight Modifier
(`docs/decision-log.md` D-007).

`r11-inline` establishes that every documented mechanism works. All six subjects share one
shape — base weight 1 plus a `weight_modifier` that should multiply it by 1,000,000 — so
draw-pool membership reads identically across them:

| Subject | Mechanism | Result |
| --- | --- | --- |
| `tech_oracle_inline_literal` | factor written literally | **drawn** — positive control holds |
| `tech_oracle_inline_lowweight` | no modifier at all | **absent** — negative control holds |
| `tech_oracle_inline_basic` | `inline_script = "path"` | **drawn** — simple expansion works |
| `tech_oracle_inline_param` | `$F$` substitution | **drawn** — parameters substitute |
| `tech_oracle_inline_nested` | inline script including another | **drawn** — inclusion nests |
| `tech_oracle_inline_override_probe` | mod file at a vanilla script's path | **drawn** — override works |

The last row answers the resolution question. Vanilla's
`technologies/rare_technologies_weight_modifiers` is federation-gated and contributes
nothing to a fresh empire; the fixture shipped an unconditional factor at that exact path
and the probe was drawn. A mod replaces a vanilla inline script by occupying its path, which
is the same-path rule of `r6` rather than any registration rule.

Nesting matters for implementation: vanilla inline scripts may themselves contain
`inline_script`, so expansion must recurse. An expander handling only one level would drop
content silently, because the game itself expands correctly and nothing would be logged.

`r12-inline-missing` establishes the failure mode. An unresolved reference **is** diagnosed,
naming the consuming file and line:

```
[inline_script_database.cpp:69]: Unknown inline_script "oracle/this_inline_script_does_not_exist"
    in file: common/technology/zz_oracle_missing_inline.txt line: 28
[technology.cpp:375]: Missing technology localization: tech_oracle_missing_inline
```

The second line is the consequential one. The technology **still registered** — it reaches
the localization and icon checks like any other definition. So the failure is not the
file-corrupting cascade that an unresolved scripted constant produces; it is quieter than
that. The definition survives, structurally valid, with the included content simply absent.

For the resolver this means a parsed definition that contained an inline reference cannot be
trusted to be complete on structure alone. Two distinct hazards follow, and only the first
is visible to the game:

- A **wrong path** is diagnosed, so the app can surface it as an Analysis Issue by watching
  for this message or by resolving paths itself.
- **Failing to expand at all** is not diagnosed by anything, because the game expands
  correctly and only the app would be skipping the step. A resolver that ignored
  `inline_script` would produce technology pages missing their weight logic entirely, with
  no error anywhere to reveal it.

That second hazard is why this row exists in the matrix rather than being treated as parser
trivia.

### Blast radius

Relevant to the [parser evaluation](./parser-evaluation.md), which must quantify how much a
malformed file costs.

- A scripted-constant failure damages **the remainder of the file**, not just the field or
  definition that used it. With two consumers in one file, the first failure caused the
  second definition to be reported as `Unexpected token`, making its result unattributable.
  Splitting them into one file each produced two clean, independent results.
- A missing technology key propagates **across files** to every definition that references
  it, and can prevent startup entirely.

## Mandatory fixtures

1. A technology defined in Vanilla and redefined by the Target Mod while omitting `potential`. — **done**, `fixtures/oracle/target`
2. Duplicate definitions of the same key in separate files within one layer. — **done**
3. Duplicate definitions in one file. — **done**
4. Exact relative-path collisions with and without a directory-replacement declaration. — **done**, `target-pathcollision` and `target-replace-path`
5. One case for every registry row in the matrix. — **done for the pre-implementation evidence phase**; named open cells require resolver-backed investigation
6. Constants that are redefined, referenced before definition, cyclic, and used in exact decimal arithmetic. — **done**, `target`, `target-risky`, `target-risky-consumed`
7. Localization and sprite collisions whose winning values are observable in the same documented entry. — deferred to the resolver integration corpus, which can project both effective values through one entry
8. ~~At least two installed DLCs that contribute colliding definitions so their effective order is observable.~~ — **withdrawn.** No DLC ships script content, so no DLC can contribute a colliding definition. See the DLC finding above.

## Completion model

### Evidence collection

The pre-implementation evidence phase is complete because:

- Every matrix row received a filesystem or game-oracle investigation.
- Every conclusion names its fixture and observation channel.
- Inconclusive observables remain inconclusive rather than being promoted into policy.
- Remaining questions require the resolver's semantic trace or integrated documentation projection to make a new discriminating observation.

### Resolution Profile acceptance

The complete Resolution Profile is accepted only when:

- Every matrix cell has one explicit policy backed by an oracle record.
- The controlled resolver reproduces every oracle result.
- Provenance distinguishes contributed, inherited, defaulted, duplicate, and shadowed facts.
- Reordering fixture creation or filesystem enumeration does not change the result.
- Unsupported or newly introduced content types fail visibly instead of inheriting another row's policy.
- A Stellaris update that changes any result produces a failing oracle test until the profile and analysis version are intentionally revised.

### Resolver conformance

The resolver module must consume the resolved policies, reproduce the captured oracle records, emit provenance in semantic resolution order, and reject unsupported cells. Its conformance work is the next phase rather than unfinished pre-implementation research.

### Current standing

| Dimension | Standing |
| --- | --- |
| Pre-implementation evidence collection | **Complete.** Every row has been investigated as far as the current external observables permit. |
| Complete Resolution Profile | **Partial.** Named open cells include megastructure field behavior, ship-component duplicate selection, scripted trigger and effect parameter behavior, and scripted-constant cross-source behavior. |
| Controlled resolver reproduces every oracle result | **Partial.** The Phase 4D core reproduces `r3`, `r6`, and `r10` — file selection, the one global path order, and both repeat rules — as machine-checked expectations over restated license-clean fixtures. Row-specific records (`r0`, `r1`, `r4`, `r8`, `r9`, `r11`–`r18`) are consumed by their own row tickets. |
| Provenance distinguishes fact kinds and semantic order | **Implemented.** Contributed, inherited, defaulted, duplicate, and shadowed, each carrying stream position, source identity, logical path, and definition ordinal; a file removed by selection records the mechanism that removed it rather than a fabricated position. |
| Deterministic enumeration | **Evidence established, and implemented for the core.** Streams are built from normalized logical paths, never from filesystem enumeration order. |
| Unsupported content types fail visibly | **Implemented.** An undeclared registry and an unresolved policy cell are typed refusals naming the cell and the observation that would settle it. A cell may be open conditionally: a row measured within one source resolves same-source collisions and refuses cross-source ones. |
| Stellaris-update comparison | **Automated for the consumed records.** The Resolution Profile pins the build every record was captured against and compares it, plus each record's artifact digests, on every ordinary test run. A re-capture blocks until the profile version and expectations are revised together. |

The evidence-collection spike is complete. The Resolution Profile is not globally complete:
the core exists and declares **no registry rows yet**, so every registry still refuses.
Resolved rows are implementation inputs; unresolved cells remain unsupported without a
fallback.

## Captured records

| Run | Tier | Subject |
| --- | --- | --- |
| `r0-baseline` | 2 | Vanilla Content with instrumentation under the recorded DLC-availability environment |
| `r1-target` | 2 | Technology, scripted trigger, effect, and constant collisions |
| `r3-replace-path` | 1 | `replace_path` over `common/technology` |
| `r4-reordered` | 2 | Enumeration-order control; winners flip with filename order |
| `r5-risky-constants` | 1 | Forward-referenced and cyclic constants, unconsumed |
| `r6-pathcollision` | 1 | Exact file-path collision |
| `r7-risky-consumed` | 1 | The same constants, consumed at load time |
| `r8-registries` | 1 | Megastructures, buildings, ship components, events |
| `r9-events-runtime` | 2 | Which event body evaluates, fired from the console |
| `r10-loadorder` | 2 | Path order versus layer order, across both registry rules |
| `r11-inline` | 2 | Inline script expansion, parameters, nesting, and path override |
| `r12-inline-missing` | 1 | An inline script reference that does not resolve |
| `r13-loc-methods` | 2 | Localisation override methods against Vanilla; log-only |
| `r14-loc-samepath` | 2 | Same-filename localisation collision; log-only |
| `r15-loc-modvmod` | 2 | Mod-against-mod localisation and `replace/` priority; log-only |
| `r16-loc-reference` | 2 | Whether localisation references see effective values; log-only |
| `r17-sprites` | 1 | Sprite collisions, read from missing-texture diagnostics |
| `r18-sprites-early` | 1 | Sprite ordering model: early-sorting mod file |

Each record holds a manifest with the installed build, activation scope, fixture tree
hashes, and artifact checksums, plus the extracted oracle facts, the normalized `error.log`,
and the technology projection where a game session existed.

The completed matrix, fixture checksums, captured evidence, and resulting decision remain in this file and are referenced by the parser and revision-bundle corpora.
