# Resolver fixtures

Corpora for the resolver's expectation suites — `src-tauri/src/analysis/resolver/oracle/` for the
record-backed rules, and `src-tauri/src/analysis/resolver/golden.rs` for the golden-case shapes that
have no record. Every file here is **original work for this repository**. No Stellaris content is
copied, and no vanilla file is reproduced.

## Why these restate the oracle records rather than reusing them

The game-oracle fixtures under [`../oracle/`](../oracle/) are frozen: their SHA-256 is
recorded in every captured record's manifest, and `tools/oracle/verify.py` enforces it, so
editing one — even a comment — silently turns every record into a claim about a file that no
longer exists. A resolver test that consumed them would either be unable to evolve or would
break the evidence it depends on.

These fixtures therefore restate the *shape* each record established, in files this suite
owns. The evidence link is the expectation table, which names the record it comes from and
pins the game build that record was captured under.

They also solve a licensing problem the oracle records do not have. `r3` and `r6` are
observations about vanilla files — a mod file landing on `common/technology/00_astral_planes_tech.txt`,
`replace_path` over the whole vanilla technology tree — and the observable was the resulting
`error.log`. Reproducing that in CI would require shipping vanilla content. A stand-in
Vanilla corpus reproduces the *rule* with no licensed bytes, and runs on a machine with no
Stellaris installed.

## The corpora

| Corpus | Restates | Shape |
| --- | --- | --- |
| `vanilla/` | the base-game file set | Two technology files and one events file, all sorting as `00_…`. `tech_contested` and `notice_contested` are the keys a mod collides with; `tech_untouched` and `notice_untouched` are the controls that separate "this file was displaced" from "resolution broke". |
| `early-mod/` | `r10-loadorder` | The same early-sorting `!!!_…` filename applied to both a replace-on-repeat and a reject-on-repeat registry. The two rules predict opposite winners, so the pair identifies the enumeration model. |
| `path-collision/` | `r6-pathcollision` | One file at `vanilla/`'s exact logical path, defining a key that file never mentions. What matters is that *both* keys the vanilla file defined disappear — merge-by-key is what this rules out. |
| `replace-path/` | `r3-replace-path` | `replace_path="common/technology"` plus one of the declarer's own files in that directory, which must still load. |
| `redefinition-vanilla/` | the base-game definitions golden case 5 redefines | A matched pair (`tech_matched_subject`, `tech_matched_control`) differing in nothing, so the redefinition can differ in exactly one field, plus `tech_untouched_baseline`. Separate from `vanilla/` because that corpus's exact key list is asserted by the r6 and r3 expectations. |
| `redefinition/` | `r1-target` | A late-sorting `zz_…` redefinition that omits `potential`, which under whole-object replacement makes the field absent rather than inherited. Adds `tech_mod_only`, the positive control proving the file contributed at all. |
| `redefinition-flipped/` | `r4-reordered`'s method, applied to technologies | The **same bytes** as `redefinition/` under an early-sorting `!!!_…` name. Only the filename differs, and the suite asserts that byte identity, so the flipped winner is a position result and not a content one. |
| `registration-vanilla/` | the base-game scripted constants Phase 4F consumes | `@base_cost` and `@shared_symbol`, declared once so a cross-source read (`@base_cost`) and a cross-source collision (`@shared_symbol`, redeclared by `constants-collision/`) both have a clean vanilla side. |
| `registration/` | `r1-target`'s trigger, effect, and constant registration cases | Duplicate scripted triggers and effects (same-file and cross-file, last wins) beside duplicate scripted constants (same-file and cross-file, first wins), plus one technology file consuming a locally-overridden, a vanilla, and a cross-file-won constant. |
| `registration-flipped/` | `r4-reordered`'s method, applied to the trigger and constant pairs | The trigger pair's two file names are swapped with each other, and the constant pair's two file names are swapped with each other — byte for byte, asserted rather than assumed — so the cross-file winner for each moves with the name while same-file results and the consumer facts are unchanged. |
| `risky-constants/` | `r5-risky-constants` and `r7-risky-consumed` | A forward reference and a two-symbol cycle, neither rejected by the game, each consumed by its own technology file so one broken reference's blast radius cannot be confused with another's. |
| `constants-collision/` | the scripted-constants cross-source open cell | Redeclares `registration-vanilla/`'s `@shared_symbol` from the Target Mod, plus one consumer of the colliding symbol and one consumer of an uncontested one. |
| `parameterized/` | the `$PARAM$` reference open cell | A scripted trigger carrying a nested `$COUNT$` substitution, one carrying a root-level `$MODE$` key, plus a parameter-free control trigger in the same file. |
| `inline-vanilla/` | the base-game side `inline/` expands against | One inline script at `technologies/rare_weight_modifiers`, gated so that expanding it contributes nothing, plus one technology naming no inline script — the scoping control that separates "expansion broke the row" from "this definition was displaced". |
| `inline/` | `r11-inline` | The record's six subjects, every one written to expand to the same shape so a subject that does not produce it did not expand: a hand-written literal positive control, a no-modifier negative control, a quoted single-argument call, a block call binding `$F$`, a block call at `inline-vanilla/`'s exact script path (with an unused binding), and — in its own file, so a nesting failure cannot reach the others — a fragment whose whole body is a second inclusion. |
| `inline-missing/` | `r12-inline-missing` | An inline reference to a path no file supplies, plus a sibling definition after it in the same file: the game diagnoses the reference and registers the technology anyway with the inclusion absent, so the resolver owes survival with an explicit fact rather than silence. |
| `events-vanilla/` | `r9-events-runtime`, `r10-loadorder` | Base event blocks using `namespace` and direct `id` keys; supplies the late and early vanilla collision sides. |
| `events/` | `r9-events-runtime` | A late mod corpus with a same-file first-wins pair and a `zz_` collision vanilla retains. |
| `events-early/` | `r10-loadorder` | The early `!!!_` event collision that registers before its vanilla counterpart. |
| `buildings-vanilla/` | `r8-registries` | The base building states `building_sets`, the field a mod replacement omits. |
| `buildings/` | `r8-registries` | A late whole-object redefinition plus a new-key control proves the mod file contributed. |
| `components/` | `r8-registries` | Clean component templates share a block label but have distinct quoted direct `key` values. |
| `components-repeat/` | `r8-registries` | A repeated direct component key reaches the intentionally unresolved duplicate-winner cell. |
| `sprites-vanilla/` | the base-game side of `r17-sprites` and `r18-sprites-early` | One sheet sprite with two Vanilla dependents whose `sprite_sheet_sprite_type` edges make the sheet winner observable. |
| `sprites/` | `r17-sprites` | Same-file, cross-file, and late cross-source replacements plus reference chains and typed missing/cyclic controls. Block labels deliberately vary so `name`, not `spriteType`, is the registry key. |
| `sprites-early/` | `r18-sprites-early` | The same sheet override under a filename sorting before Vanilla's `alerts.gfx`, so the Target Mod definition is read and then displaced. |
| `localization-vanilla/` | the base-game side of `r13-loc-methods` and `r14-loc-samepath` | Two localization files: one a mod collides with, holding four keys of which the winner restates one, and one untouched control proving shadowing stays scoped to the losing file. The colliding file carries a byte-order mark and the control is mark-less and CRLF, so both real encodings reach ingestion through a committed corpus and not only through inline test literals. |
| `localization-methods/` | `r13-loc-methods`, plus `r15-loc-modvmod`'s `replace/` phase | An early ordinary file, a late ordinary file, and an early-named `replace/` file prove source phase and replacement phase outrank filename order. Each contests a key the vanilla corpus states, which is what makes the r13 expectation key-level rather than only file-level. |
| `localization-samepath/` | `r14-loc-samepath` | One Target Mod file at the Vanilla technology-localization path, restating one of that file's four keys. The resolver stops at whole-file selection; ingestion is where the other three become raw-key casualties. |
| `localization-modvmod/` | `r15-loc-modvmod`, key level | Four files in three trees — Vanilla, an earlier-loading mod's ordinary and `replace/` files, and a later-loading mod's ordinary file. **Not a corpus**: production supplies one Target Mod rank, so no `SourceSnapshot` pair can hold two mods, and these are fed to localization ingestion in the order the record measured. That the phase sorter *produces* that order from two ranks is pinned separately, against the same production sorter, by `stream::tests::r15_preserves_enabled_mod_order_and_moves_every_replace_file_last`. The record's claim is the composition of the two halves; neither may be read as the other. |

## The golden-case corpora

The six below are the exception to this file's framing: they restate a **golden case** from
[`docs/mvp-acceptance.md`](../../docs/mvp-acceptance.md) rather than an oracle record, and no
captured observation of the game anchors them. Their expectations live in
`src-tauri/src/analysis/resolver/golden.rs` for that reason — `oracle/` is where a claim rests on a
record — and each one states there what it can honestly assert before Phase 6 exists.

Golden case 5's corpora are `redefinition*/` above, which do rest on records (`r1`, `r4`, `r10`).
Golden case 1 needs no fixture: its subject is a pinned *vanilla* technology, reached through the
drift-checked local-corpus run, and named in `docs/decision-log.md` (D-132).

| Corpus | Restates | Shape |
| --- | --- | --- |
| `malformed-vanilla/` | golden case 4's untouched side | One key defined here and contested by nothing, so "successfully analyzed content remains searchable" has a definition no mod-side fault could have reached. |
| `malformed/` | golden case 4 | Three files whose faults cost three different things. `malformed_recovery.txt` loses a definition outright to an unclosed brace and downgrades what follows it to Recovered; `malformed_stray_brace.txt` loses no definition and downgrades anyway; `malformed_intact.txt` is wholly clean, which is what separates "this file recovered" from "the corpus broke". |
| `zero-weight-vanilla/` | golden case 2's scoping control | One uncontested key with no `weight_modifier` at all, so the `×0` result is provably scoped to one modifier of one definition. |
| `zero-weight/` | golden case 2 | A matched pair differing in exactly one token — `factor = 0` against `factor = 0.5` on the same gated modifier — plus the prerequisite decoy `D-008` names, which shares the subject's other modifiers and its name stem and must never carry the zero. |
| `enigmalith-vanilla/` | golden case 3's base values | The scripted constants the technologies read across sources: a zero draw weight, a nonzero one for the control, and a base cost. Declared here so the read is genuinely cross-source, and redeclared nowhere so the constants row's open cross-source collision cell is not reached. |
| `enigmalith/` | golden case 3 | A zero base Draw Weight fed by a constant rather than stated inline, its matched nonzero control, two events granting the same technology from *different* enclosing actions (an `immediate` and an `option`) plus one that grants nothing, and a megastructure entry the megastructures row refuses to interpret on its open field cell — asserted both as present at the parser seam and as visibly refused at the resolver's. |

## Reading them

Each corpus is loaded through `include_bytes!` into
`source::fixture::FixtureCorpus`, which applies the real enumeration policy. That is a
compile-time read: nothing here is traversed at test time, so the suite has no dependency on
a filesystem layout or a host.
