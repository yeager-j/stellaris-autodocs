#!/usr/bin/env python3
"""Activate one oracle run configuration.

Materializes the fixture mods for a run into the Stellaris mod directory, writes their
descriptors, and sets `dlc_load.json` to enable exactly those mods in exactly that order.
The game executable reads `dlc_load.json` directly, so activation scope is a recorded file
rather than a sequence of launcher clicks nobody can reproduce.

    python3 tools/oracle/activate.py r0-baseline
    python3 tools/oracle/activate.py restore
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys

from oracle_paths import (
    BACKUP,
    DESCRIPTOR_PREFIX,
    DLC_LOAD,
    LOGS_DIR,
    MOD_DIR,
    STAGING,
    fixture_files,
    hash_tree,
    load_runs,
    sha256_file,
)


def refuse_if_running() -> None:
    """Refuse to activate while Stellaris is running.

    Activation truncates the logs and rewrites the staged fixtures. Doing that under a live
    game destroys the evidence it is about to produce and leaves the process running script
    loaded from one fixture version while the files on disk are another — a run that looks
    valid and is not.

    This happened once: a re-activation emptied the logs of a game that had already been
    launched, and the probe output went into a file that had just been zeroed.
    """
    # `pgrep -x` matches the executable name exactly. `pgrep -f` was used first and matched
    # this harness's own shell commands, because their command lines mention the game's path
    # while checking for it. That guard would have refused to activate because of itself.
    try:
        found = subprocess.run(
            ["pgrep", "-x", "stellaris"],
            capture_output=True,
            text=True,
        ).stdout.split()
    except FileNotFoundError:
        return
    if found:
        raise SystemExit(
            f"Stellaris is running (pid {', '.join(found)}).\n"
            "Activation clears logs and rewrites staged fixtures, which would invalidate "
            "that session.\nQuit the game first, then activate."
        )


def back_up_once() -> None:
    """Preserve the user's real mod configuration before the first activation.

    Written once and never overwritten: a second activation must not capture an
    already-activated state as if it were the original.
    """
    if BACKUP.exists() or not DLC_LOAD.exists():
        return
    shutil.copy2(DLC_LOAD, BACKUP)
    print(f"backed up {DLC_LOAD.name} -> {BACKUP.name}")


def clear_logs() -> None:
    """Empty the game's logs before a run.

    Stellaris appears to truncate its logs at startup, but the harness must not depend on
    that: if any log carried over, the previous run's diagnostics would be captured as this
    run's and every difference between two runs would be noise. Truncating in place rather
    than deleting keeps whatever file handles or permissions the game expects.
    """
    if not LOGS_DIR.is_dir():
        return
    cleared = 0
    for log in LOGS_DIR.glob("*.log"):
        log.write_text("")
        cleared += 1
    print(f"cleared {cleared} log files")


def clear_staging() -> None:
    if STAGING.exists():
        shutil.rmtree(STAGING)
    for descriptor in MOD_DIR.glob(f"{DESCRIPTOR_PREFIX}*.mod"):
        descriptor.unlink()


def use_installed(descriptor_name: str) -> tuple[str, dict[str, str]]:
    """Enable an already-installed mod by its existing descriptor, without staging it.

    Mod-against-mod resolution needs a real second mod, and copying a Workshop mod into
    staging would be both wasteful and a redistribution problem. The descriptor is hashed so
    the record still pins which build of that mod a run used; the mod's own content is left
    where Steam put it.
    """
    descriptor = MOD_DIR / descriptor_name
    if not descriptor.exists():
        raise SystemExit(f"installed mod descriptor not found: {descriptor}")
    return f"mod/{descriptor_name}", {descriptor_name: sha256_file(descriptor)}


def materialize(name: str) -> tuple[str, dict[str, str]]:
    """Copy one fixture into staging, applying its variant derivation if it has one.

    Variants carry no content of their own. `target-replace-path` and `target-reordered`
    are derivations of `target`, so the content has exactly one authority and the two runs
    cannot drift apart while appearing to test the same thing.
    """
    try:
        sources = fixture_files(name)
    except KeyError as unknown:
        raise SystemExit(str(unknown))

    destination = STAGING / name
    for relative, path in sources.items():
        staged = destination / relative
        staged.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, staged)

    # variant.json describes the fixture; it is not content the game should read.
    (destination / "variant.json").unlink(missing_ok=True)

    descriptor_body = (destination / "descriptor.mod").read_text()
    if not descriptor_body.rstrip().endswith('"'):
        descriptor_body = descriptor_body.rstrip() + "\n"
    descriptor_body = descriptor_body.rstrip() + f'\npath="{destination}"\n'

    descriptor_path = MOD_DIR / f"{DESCRIPTOR_PREFIX}{name}.mod"
    descriptor_path.write_text(descriptor_body)

    return f"mod/{descriptor_path.name}", hash_tree(destination)


def activate(run_id: str) -> None:
    runs = load_runs()
    if run_id not in runs["runs"]:
        raise SystemExit(
            f"unknown run: {run_id}\nknown runs: {', '.join(runs['runs'])}"
        )
    run = runs["runs"][run_id]

    refuse_if_running()
    back_up_once()
    clear_logs()
    clear_staging()
    STAGING.mkdir(parents=True, exist_ok=True)

    # Order here IS load order: dlc_load.json's enabled_mods list is what the game reads.
    # For mod-against-mod tests that ordering is the independent variable, so it must come
    # from the run definition and never be sorted or deduplicated on the way through.
    enabled, trees = [], {}
    for name in run["mods"]:
        if name.startswith("installed:"):
            entry, tree = use_installed(name.split(":", 1)[1])
        else:
            entry, tree = materialize(name)
        enabled.append(entry)
        trees[name] = tree

    # Every DLC stays enabled. A run must not silently differ from the recorded DLC set.
    DLC_LOAD.write_text(json.dumps({"enabled_mods": enabled, "disabled_dlcs": []}))

    (STAGING / "activation.json").write_text(
        json.dumps(
            {"run": run_id, "enabled_mods": enabled, "fixture_hashes": trees},
            indent=2,
            sort_keys=True,
        )
    )

    print(f"activated {run_id} (tier {run['tier']})")
    for entry in enabled:
        print(f"  {entry}")
    print()
    print(run["purpose"])
    print()
    print(runs["tiers"][str(run["tier"])])


def restore() -> None:
    clear_staging()
    if BACKUP.exists():
        shutil.copy2(BACKUP, DLC_LOAD)
        BACKUP.unlink()
        print(f"restored {DLC_LOAD.name} from backup")
    else:
        print("no backup present; staging cleared")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    if sys.argv[1] == "restore":
        restore()
    else:
        activate(sys.argv[1])


if __name__ == "__main__":
    main()
