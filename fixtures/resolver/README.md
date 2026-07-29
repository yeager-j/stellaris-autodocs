# Resolver fixtures

Corpora for the resolver's oracle-expectation suite
(`src-tauri/src/analysis/resolver/oracle/`). Every file here is **original work for this
repository**. No Stellaris content is copied, and no vanilla file is reproduced.

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

## Reading them

Each corpus is loaded through `include_bytes!` into
`source::fixture::FixtureCorpus`, which applies the real enumeration policy. That is a
compile-time read: nothing here is traversed at test time, so the suite has no dependency on
a filesystem layout or a host.
