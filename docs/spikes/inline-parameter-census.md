# Inline-script parameter census

Status: Complete. Measured against Stellaris `Pegasus v4.4.6 (fdde)` (`modsCompatibilityVersion` `4.4`) and Ancient Cache of Technologies, Workshop `1419304439`. Settles STE-34; the decision it produces is D-132.

## The question

The dialect lexer classifies a token as `ScalarKind::Parameter` only when `$` is both the first and last byte (`src-tauri/src/analysis/parser/jomini.rs`, `classify`). Phase 4G's expander therefore substitutes whole-token `$PARAM$` and nothing else, and an **embedded** shape — `tech_$TIER$`, `"CONTRACT_$KEY$_PROJECT"`, `@base_army_count_$TYPE$` — reaches an effective field as literal text with **no typed fact attached**. It is the one place the inline-script mechanism is silent about a shape it did not handle. D-131 covers the two shapes that *are* typed-unresolved; this one was not even detected.

Two things follow, and only measurement separates them. If the shape does not occur where a row that expands inline scripts can reach it, the pass-through is vacuous and the silence costs nothing today. If it does occur there, the resolver is publishing fabricated identifiers — a `has_technology = tech_$TIER$` that names no technology — with nothing anywhere to reveal it.

## Method

`cargo test --features test-support embedded_parameter_census -- --ignored --nocapture`

The run lives at `src-tauri/src/analysis/resolver/census.rs`; its module comment is the contract. Two properties of the method matter more than the counts:

**Tokenization is the lexer's decision, not the census's.** Every fragment is read through the production parser and each scalar is classified by the `ScalarKind` the lexer assigned it. A regex over the file bytes would be a second authority on what a token is, and it would also count the `$PARAM$` occurrences in `common/inline_scripts/00_README.txt`'s prose — which the parser discards with the comments they sit in. The census's own token reading is confined to one question the lexer does not answer: how many closed `$…$` runs a token contains, so that a bare `$` is not mistaken for a parameter shape (`parameter_runs`, with its own unit test).

**Reachability is the production expander's decision, not a second include-graph walk.** The technologies row is resolved over both corpora and the reachable fragment set is read out of the `InlineScriptFact`s it recorded. `expand` records one fact per inclusion site at every nesting depth, so the `Expanded` facts' script sites *are* the transitive closure — produced by the same mechanism that would have to substitute an embedded parameter if one were there.

The fragment set is the *surviving* one: `stream::build` over `inline_scripts::SCOPE` after file selection, so a mod file at a vanilla fragment's path is counted once, as the winner.

## What the corpus holds

624 surviving fragments — Vanilla's 601 plus ACOT's 29, less 6 same-path replacements under `common/inline_scripts/districts/`.

| Lexer kind | Placement | Occurrences | Distinct (token, fragment) |
| --- | --- | ---: | ---: |
| `Parameter` | whole token | 1,469 | 582 |
| `Unquoted` | embedded | 657 | 431 |
| `Quoted` | embedded | 69 | 63 |
| `VariableRef` | embedded | 63 | 56 |
| **embedded, total** | | **789** | **550** |

No occurrence of any other kind: no `VariableExpr`, `Number`, or `ScopeToken` carries a parameter run, and no fragment quotes a *whole-token* parameter (`"$NAME$"`), which would be a second shape invisible to substitution for a different reason — a quoted token is `ScalarKind::Quoted` whatever its bytes say.

**So the shape is real and abundant.** Representative occurrences, chosen for the positions they cover — every one is a verbatim line from the corpus:

| Source line | Fragment | Position |
| --- | --- | --- |
| `planet_$JOB$_$RESOURCE$_produces_add = $AMOUNT$` | `buildings/planet_job_resource_produces_add` | field **key**, two runs, alongside a whole-token value |
| `has_country_flag = $EVENT$_triggered` | `shroud/shroud_forged_random_events_exclusion` | value, run **leading** the token |
| `key = EVASION_GUN_$TIER$` | `ship_components/bio_weaver_evasion_weapons` | value — and the spliced definition's own **identifier** |
| `name = "CONTRACT_$KEY$_TRAVEL_PROJECT"` | `contracts/ContractTravelProjectEnable` | **quoted** value, where the lexer's kind is `Quoted` regardless of the bytes |
| `count = @base_army_count_$TYPE$_$TIME$` | ACOT `ai_helper/ai_helper_create_army_loops` | inside an **`@` reference**, so two substitution mechanisms meet in one token |
| `mult = owner.value:fe_building_cap\|BUILDING\|$BUILDING$\|` | `buildings/fallen_empire_building_limits` | inside an inline **value expression** |
| `district_acot_swap_dm_$TYPE$$TRAILER_TYPE_LEADING_UNDERSCORE$ = {` | ACOT `districts/acot_vanilla_districts/acot_district_swap_generator` | **two adjacent** runs and nothing between them |

The third is the one worth pausing on: `key = EVASION_GUN_$TIER$` is not a weight the row could publish slightly wrong, it is the spliced definition's *identifier*. A row that expanded this fragment without substituting would register a ship component literally keyed `EVASION_GUN_$TIER$` — a key rule producing a name nothing references and no localization resolves.

The 550 distinct embedded occurrences, by the directory the fragment sits in, heaviest first: `grand_archive` 119, `contracts` 72, `shroud` 71, `ship_components` 65, `trait` 60, `events` 30, `special_projects` 26, `pop_faction_types` 26, `paragon` 16, `patron_accords_messages` 12, then a tail of fifteen directories at 6 or fewer — including a single one under `technologies/` (below).

## What the technologies row reaches

1,436 resolved technologies state 158 inclusion sites. **All 158 expanded**; no site produced any `UnresolvedInline` variant over either corpus. They reach four distinct fragments:

```
common/inline_scripts/technologies/cosmic_storms_technologies_cost_modifiers.txt
common/inline_scripts/technologies/cosmic_storms_technologies_weight_modifiers.txt
common/inline_scripts/technologies/rare_technologies_weight_modifiers.txt
common/inline_scripts/technology/archaeotech_weight.txt
```

Those four hold **one** parameter occurrence between them: the whole-token `$TECHNOLOGY$` in `rare_technologies_weight_modifiers`, which is the shape `r11` measured and which the expander substitutes. **Zero embedded occurrences. Zero conditional blocks.**

The near miss is worth naming. `common/inline_scripts/technologies/` holds a fourth file, `add_experimental_testing_tech_progress.txt`, which carries the embedded `has_$TECH$_prerequisites` — and no technology includes it. The shape sits one directory-mate away from the row that would publish it, which is why the conclusion below is stated as a fact about the corpus at a pinned build rather than as a property of the mechanism.

## Conclusion

**Whole-token substitution covers the technology-reachable corpus completely, so the silent pass-through is vacuous at this build — and it is one fragment away from not being.**

That is a narrower claim than "the corpus does not use embedded parameters", which is false: 789 occurrences say otherwise. The pass-through is vacuous for the *only row that expands inline scripts today*. `scripted-triggers`, `scripted-effects`, and the Phase 4H rows all hold `InlineScript` at `DetectedNotResolved`, so none of them expands a fragment yet; the 65 distinct embedded occurrences under `ship_components` and the 119 under `grand_archive` become live the moment one of those cells flips.

Nothing else in the corpus can see the shape either. Reference detection reads `ScalarKind` too (`registry::Scan::walk_scalar` records `ReferenceKind::Parameter` from `ScalarKind::Parameter` only), so an embedded run in an `Unquoted` token is invisible to the undeclared-kind refusal as well. The technologies row does not declare `ReferenceKind::Parameter`, and that non-declaration is exactly why a *whole-token* leftover has to be omitted (`UnboundParameter`) while an embedded one passes through unremarked. The two shapes differ in visibility, not in seriousness.

### A secondary finding: conditional blocks do not occur in fragments at all

The census counts `[[PARAM] … ]` blocks over the same walk. **Zero, in all 624 fragments.** Within `common/`, conditional blocks appear only under `scripted_effects` (11 files), `scripted_triggers` (2), and `script_values` (1) — never under `inline_scripts`. So D-131's `ConditionalUnmeasured` is vacuous over the current corpus for the same reason and by the same measurement. This changes nothing about that variant, which is the honest omission for a shape no record settles; it records that the omission costs no content today.

### Why the assertions, and why none of them passes vacuously

The counts above are printed, not pinned: they are a property of whichever build is installed, and this note is where a specific reading belongs. Three claims are asserted instead, because each would be a changed *conclusion*:

1. **No technology-reachable fragment carries an embedded shape.** The finding. If it fails, the pass-through is live for a shipped row.
2. **The corpus at large carries many.** The census's own negative control: the same detector that reports zero above finds hundreds elsewhere, so the zero is a fact about reachability rather than a detector matching nothing.
3. **The reachable set contains the whole-token `$TECHNOLOGY$` `r11` measured.** Without it, a reachability computation that returned nothing would satisfy claim 1 by having no fragments to look in.

Claim 1 has been shown to go red. Widening the reachable set to every surviving fragment — one edit, reverted — produces:

```
assertion `left == right` failed: an embedded parameter is now reachable from a technology, so
`substitute`'s silent pass-through is live for a shipped row. D-132 conditions a typed
`UnresolvedInline` variant on exactly this. 550 distinct embedded occurrences, first 10:
  [Quoted] $COMPONENT_SET$_TOOLTIP <- common/inline_scripts/grand_archive/mutations/camouflage/camouflage_mutation.txt
  …
  left: 789
 right: 0
```

## Decision

Recorded as D-132. In short: `UnresolvedInline` gains `EmbeddedParameterUnmeasured { token }`; STE-34 does not implement it, because STE-34 touches no resolver code and the census bounds the interval.

## What would settle the substitution rule: r19-inline-embedded (drafted)

Not required by STE-34 — its capture-plan criterion is conditioned on embedded shapes occurring in fragments technologies consume, and they do not. Drafted anyway, because a row on the roadmap will reach one, and because a plan is cheaper to write while the corpus evidence is in hand than to reconstruct later.

Method follows `r11-inline` exactly: the `oracle_probe` mod for the `probe_loaded` / `probe_complete` canaries, one `oracle_target-embedded` mod holding the subjects, activation through `dlc_load.json`, a real game start, and the `country.<id>.tech_status.potential` projection as the observation. Every subject uses `r11`'s single readable shape — base weight 1 plus a `weight_modifier` that multiplies it by 1,000,000 — so draw-pool membership reads the same way for all of them.

The discriminator is a country flag the probe sets at game start, not a starting technology. A fragment carrying `has_country_flag = flag_$NAME$` with `NAME = oracle` bound is true only if the game substituted inside the larger token: substituted, the trigger reads `flag_oracle`, the weight multiplies, and the technology appears in `potential`; not substituted, the trigger names a flag nothing sets and the technology does not appear. The two readings are therefore distinguishable from one projection, and the subject does not depend on which technologies an empire happens to start with.

Subjects, each one technology including one fragment:

| # | Fragment shape | Call | What the outcome settles |
| --- | --- | --- | --- |
| 1 | `has_country_flag = flag_$NAME$` | `NAME = oracle` | Whether the game substitutes inside a larger unquoted token at all — the shape's substitution rule |
| 2 | The same fragment | no binding | Whether an unbound embedded parameter is a game error, a literal pass-through, or a dropped line. The counterpart of `UnboundParameter`, which D-131 left open |
| 3 | `has_country_flag = "flag_$NAME$"` | `NAME = oracle` | Whether quoting changes the answer. The resolver cannot infer this, because quoting erases the lexer's parameter classification entirely |
| 4 | `factor = @weight_$TIER$`, with `@weight_1 = 1000000` in a scripted-variables file | `TIER = 1` | Which mechanism runs first: whether the parameter is substituted before the constant is looked up. The ACOT shape; Phase 4F owns the other half |

`error.log` is captured and normalized for every subject, because a diagnostic naming the consuming file and line is what `r12` established the game emits for an inclusion it cannot satisfy — its presence or absence is evidence in its own right, and subject 2 may produce nothing else.

Subject 4 is worth capturing even if 1 through 3 come out as expected: it is the only subject whose answer the others do not imply, and the only one two mechanisms share.
