# Resolver oracle fixtures

Fixture mods for the [resolver evaluation](../../docs/spikes/resolver-evaluation.md) game
oracle. They establish what Stellaris actually does when definitions collide, so the
Resolution Profile records observed behavior instead of assumed behavior.

## Design

Each measurement is a **difference between two runs**, never a single reading. A lone
observation cannot separate "the mod's definition won" from "vanilla already behaved this
way", which is exactly the substitution of assumption for evidence the spike forbids.

The corpus is split into instrumentation and subject:

| Fixture | Role |
| --- | --- |
| `probe` | Instrumentation. Present in every run, byte-identical in every run. Emits the canary and probes only vanilla identifiers. |
| `target` | The main subject. Technology key collisions, scripted trigger and effect duplicates, scripted constant redefinition. |
| `target-pathcollision` | Exact file-path collision. **Crashes the game during startup, and that is its evidence.** |
| `target-replace-path` | `target`'s content with a `replace_path` descriptor. |
| `target-reordered` | `target`'s content with the `_a` / `_b` filenames swapped. |
| `target-risky` | Forward-referenced and cyclic constants, declared but never consumed. |
| `target-risky-consumed` | The same constants, read from a technology `cost` field. |

`target-replace-path` and `target-reordered` hold no content of their own. Each is a
`variant.json` describing a derivation from `target`, resolved by `fixture_files()` in
`tools/oracle/oracle_paths.py`. Both activation and verification go through that one
function, so a variant cannot mean one thing when it is staged for the game and another
when its captured record is checked. Two copies of the same content would be two
authorities that drift.

### Isolation

Three fixtures exist only because a case can fail catastrophically and take a run with it:

- `target-pathcollision` first lived inside `target`. Shadowing the vanilla file removed two
  technologies that roughly sixty vanilla files reference, startup died, and every other
  measurement in that run — triggers, effects, constants, technology redefinition — was
  lost. One unrelated case had silently become the run's result.
- `target-risky-consumed` splits its two consumers into one file each. When they shared a
  file, the forward-reference failure cascaded and the next definition was reported as an
  unexpected token, making the cycle result unattributable.

A case that can fail catastrophically gets its own mod and its own run.

## Reading the evidence

Probes emit `ORACLE|<case>|<fact>|<value>` to `logs/game.log`. Values are reported by
branching on a comparison rather than by interpolating a number, because a taken branch is
unambiguous evidence of the compared value while an interpolated number would additionally
have to format the way the harness expects.

Three facts are canaries — `probe_loaded`, `probe_complete`, `target_loaded`. If a canary
is missing, the logging channel did not work and no *absent* value from that run may be
interpreted: a fact that did not happen and a channel that did not deliver look identical
without them.

Effective technology eligibility is not read from logs. It comes from the save's exported
`country.<id>.tech_status.alternatives`, the engine's own research draw pool. Subjects carry
a dominating weight so "drawable" reliably means "drawn".

### Vestigial content in `probe`

`probe` still grants `tech_colossus`, `tech_rift_sphere`, and `tech_astral_harvesting`, and
carries comments about `tech_pk_cracker` and a commented-out `colossus_project` flag. Those
belong to an earlier subject choice that was replaced by the `tech_adaptive_combat_algorithms`
matched pair.

The file is deliberately **not** cleaned up. Its SHA-256 is recorded in every captured
record's manifest, including `r0-baseline`. Editing it — even a comment — would break the
link between the evidence and the fixture that produced it, and every record would silently
become a claim about a file that no longer exists. `tools/oracle/verify.py` enforces this.

The grants are harmless: they are identical in every run, so they cannot explain a
difference between runs.

## Licensing

Every file here is original work for this repository. No Stellaris content is copied.
Fixtures reference vanilla identifiers such as `tech_adaptive_combat_algorithms` by name,
and comments quote short field snippets where needed to explain what is being tested, but no
vanilla file is reproduced. Reproducing a run requires a licensed local Stellaris
installation.

## Reproducing

```bash
python3 tools/oracle/activate.py r0-baseline
# launch Stellaris, follow the run's tier instructions
python3 tools/oracle/capture.py r0-baseline
python3 tools/oracle/diff.py r0-baseline r1-target
```

Run configurations live in `tools/oracle/runs.json`. `verify.py` checks every captured
record against the committed fixtures and the installed game build. `activate.py restore`
returns the game's mod configuration to the backup taken on first activation.
