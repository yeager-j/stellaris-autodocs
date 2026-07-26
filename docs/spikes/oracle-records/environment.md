# Oracle environment record

The pinned environment for every run under this directory. Each run's `manifest.json` also
records the installed version at capture time, so a record taken against a different build
is visible rather than silently mixed in.

## Build

| Fact | Value |
| --- | --- |
| Marketing version | `Pegasus v4.4.6` |
| Raw version | `v4.4.6` |
| Mods compatibility version | `4.4` |
| Distribution | Steam |
| Operating system | macOS, Darwin 25.5.0 |
| Architecture | arm64 |

Read from `<install>/launcher-settings.json`, not from a log banner: `game.log` records the
last run rather than the installed build, and the two disagree whenever logs are stale.

## DLC

All 29 installed DLC directories are enabled for every run. `activate.py` writes an empty
`disabled_dlcs`, so no run can silently differ from this set.

```
dlc002_arachnoid   dlc004_plantoid    dlc012_leviathans  dlc013_horizon_signal
dlc014_utopia      dlc015_anniversary dlc016_synthetic_dawn dlc017_apocalypse
dlc018_humanoids   dlc019_distant_stars dlc020_megacorp  dlc021_ancient_relics
dlc022_lithoids    dlc023_federations dlc024_necroids    dlc025_nemesis
dlc026_aquatics    dlc027_overlord    dlc028_toxoids     dlc029_firstcontact
dlc030_paragon     dlc031_astral_planes dlc032_machine_age dlc033_cosmic_storms
dlc034_grand_archive dlc035_rick_the_cube dlc036_biogenesis dlc037_shadows_shroud
dlc038_infernals   dlc039_stargazer
```

The user's own configuration disabled `dlc033_cosmic_storms`. The harness re-enables it and
restores the original file through `activate.py restore`.

## Activation

Mods are activated by writing `dlc_load.json` directly and launching the game executable.
The Paradox launcher and its playset database are not involved, so a run's activation scope
is a recorded file rather than a sequence of UI actions that cannot be reproduced or
audited.

```bash
python3 tools/oracle/activate.py <runId>
```

`activate.py` backs up the original `dlc_load.json` once, truncates every file in
`<data>/logs`, materializes the run's fixtures under `<data>/mod/oracle/`, writes their
descriptors, and records the fixture tree hashes in `activation.json`.

Logs are truncated because the evidence is a difference between runs. A log that carried
over from a previous run would contribute that run's diagnostics to this one, and the
difference would be noise.

## Launch

```bash
"$HOME/Library/Application Support/Steam/steamapps/common/Stellaris/stellaris.app/Contents/MacOS/stellaris" -gdpr-compliant
```

## Pinned new-game setup for tier 2 runs

Every tier 2 run must use the same setup, or an empire difference will be read as a
resolution difference.

| Setting | Value |
| --- | --- |
| Empire | United Nations of Earth (default, unmodified) |
| Galaxy size | Tiny |
| AI empires | 0 |
| Fallen empires | 0 |
| Marauder empires | 0 |
| Advanced AI starts | 0 |
| Crisis strength | default |
| Difficulty | Ensign |
| Ironman | off |

After the game starts, save as `oracle_<runId>` and quit. Letting in-game time pass is not
required: the research draw is made during galaxy generation from the effective
definitions. Saves taken at `2200.01.01` (`r9`, `r10`) carry a fully populated
`alternatives` block identical to those taken a month later (`r1`, `r4`).

Runs that use the console open it with the backtick key, and require Ironman to be off.

Use the stock United Nations of Earth from the empire list, not a saved custom empire.
Saved designs on this machine reference modded origins and flags, and selecting one would
change the empire under test.

The technology measurement depends on the probe empire being an ordinary biological empire:
`tech_adaptive_combat_algorithms` is gated by `is_machine_empire`, and that gate being
closed is what makes the omitted-`potential` result readable.

## Capture

```bash
python3 tools/oracle/capture.py <runId>
python3 tools/oracle/diff.py r0-baseline <runId>
```

`capture.py` refuses to run if the currently activated configuration is not the one being
captured, so a run's evidence cannot be filed under another run's name.

## Restoring the machine

```bash
python3 tools/oracle/activate.py restore
```

Removes the materialized fixtures and their descriptors and restores the original
`dlc_load.json`. Saves created during runs are left in place; delete them by hand if
unwanted.
