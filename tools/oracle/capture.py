#!/usr/bin/env python3
"""Capture the evidence produced by one oracle run.

    python3 tools/oracle/capture.py r0-baseline

Writes to docs/spikes/oracle-records/<runId>/:

    manifest.json         environment, activation, fixture and artifact hashes
    oracle-facts.txt      the ORACLE| lines from game.log, in emission order
    error.normalized.txt  error.log with timestamps stripped, deduplicated, sorted
    tech-status.json      the player country's exported technology sets (tier 2 only)

Raw saves are not copied. They are tens of megabytes and derived from proprietary content;
the projection plus the recorded checksums is what a licensed installation needs to
reproduce the result.
"""

from __future__ import annotations

import json
import re
import shutil
import sys
from collections import Counter
from pathlib import Path

import gamestate
from oracle_paths import (
    DLC_LOAD,
    LOGS_DIR,
    RECORDS,
    SAVES_DIR,
    STAGING,
    hash_tree,
    installed_version,
    load_runs,
    sha256_file,
)

TIMESTAMP = re.compile(r"^\[\d{2}:\d{2}:\d{2}\]")
# Bounded to one line. A negated character class matches newlines, so an unbounded pattern
# swallows the rest of the file into a single "fact".
ORACLE_LINE = re.compile(r"ORACLE\|[^\"\n]*")
# The game prefixes each log effect with the file and line that emitted it. That is free
# provenance: it proves which fixture file produced a fact, which matters when the question
# is which of two colliding files won.
EMITTER = re.compile(r"file: (\S+) line: (\d+)\.")
# Which canaries a run must show. The default pair comes from the probe mod's
# on_game_start_country hook, which only fires when a game is CREATED. A run that loads an
# existing save never fires it and must declare the canaries its own probe emits, or every
# such run reports a dead channel while its evidence sits in the log.
DEFAULT_CANARIES = ("probe_loaded", "probe_complete")


def normalize_errors(text: str) -> tuple[list[str], int]:
    """Strip timestamps and collapse repeats.

    Timestamps differ on every run and would dominate a diff; repeat counts are kept
    because "reported once" and "reported four hundred times" are different facts.
    """
    counts: Counter[str] = Counter()
    total = 0
    for line in text.splitlines():
        total += 1
        counts[TIMESTAMP.sub("", line).strip()] += 1
    return [f"{count}\t{line}" for line, count in sorted(counts.items())], total


def extract_facts(text: str) -> list[str]:
    """One fact per line, as `<emitting file>:<line>\t<fact>`.

    Emission order is preserved. It is not itself evidence, but a fact appearing before the
    canary that should precede it is a sign the run is not what it appears to be.
    """
    facts = []
    for line in text.splitlines():
        match = ORACLE_LINE.search(line)
        if not match:
            continue
        fact = match.group(0).rstrip('"').strip()
        emitter = EMITTER.search(line)
        origin = f"{emitter.group(1)}:{emitter.group(2)}" if emitter else "?"
        facts.append(f"{origin}\t{fact}")
    return facts


def find_save(run_id: str) -> tuple[Path | None, list[str]]:
    """The most recent save matching the run id, plus any others with the same name.

    Re-running a configuration creates a second save under a new empire folder with the
    same file name. Selecting by name alone picks whichever sorts first, which is the
    earlier attempt — the run that was superseded precisely because something was wrong
    with it. Selection is by modification time, and every rejected candidate is reported so
    a stale pick is visible rather than silent.
    """
    matches = sorted(
        SAVES_DIR.rglob(f"*{run_id}*.sav"), key=lambda p: p.stat().st_mtime
    )
    if not matches:
        return None, []
    chosen = matches[-1]
    superseded = [str(p.relative_to(SAVES_DIR)) for p in matches[:-1]]
    return chosen, superseded


def capture(run_id: str) -> None:
    runs = load_runs()
    if run_id not in runs["runs"]:
        raise SystemExit(f"unknown run: {run_id}")
    run = runs["runs"][run_id]

    activation_file = STAGING / "activation.json"
    if not activation_file.exists():
        raise SystemExit("no active oracle run; run activate.py first")
    activation = json.loads(activation_file.read_text())
    if activation["run"] != run_id:
        # Capturing under the wrong activation would attribute one configuration's
        # evidence to another, which is worse than capturing nothing.
        raise SystemExit(
            f"active run is {activation['run']}, not {run_id}; re-activate before capturing"
        )

    out = RECORDS / run_id
    out.mkdir(parents=True, exist_ok=True)

    manifest: dict = {
        "run": run_id,
        "tier": run["tier"],
        "purpose": run["purpose"],
        "stellaris": installed_version(),
        "activation": {
            "enabled_mods": activation["enabled_mods"],
            "dlc_load": json.loads(DLC_LOAD.read_text()),
        },
        "fixture_hashes": activation["fixture_hashes"],
        "artifacts": {},
        "warnings": [],
    }

    game_log = LOGS_DIR / "game.log"
    if game_log.exists():
        text = game_log.read_text(errors="replace")
        facts = extract_facts(text)
        (out / "oracle-facts.txt").write_text("\n".join(facts) + "\n")
        manifest["artifacts"]["oracle-facts.txt"] = sha256_file(out / "oracle-facts.txt")
        manifest["fact_count"] = len(facts)
        manifest["game_log_banner"] = next(
            (line for line in text.splitlines() if "Game Version" in line), None
        )

        # The canary is the negative control for the logging channel. Without it, a fact
        # that did not happen and a channel that did not deliver are indistinguishable, so
        # every absent-value conclusion from this run would be unfounded.
        expected = tuple(run.get("canaries", DEFAULT_CANARIES))
        present = {c for c in expected if any(c in fact for fact in facts)}
        manifest["canaries"] = {c: (c in present) for c in expected}
        if run["tier"] == 2 and present != set(expected):
            manifest["warnings"].append(
                "CANARY MISSING: the log channel did not deliver. No absent-value result "
                "from this run may be interpreted."
            )
    else:
        manifest["warnings"].append("game.log not found")

    error_log = LOGS_DIR / "error.log"
    if error_log.exists():
        normalized, total = normalize_errors(error_log.read_text(errors="replace"))
        (out / "error.normalized.txt").write_text("\n".join(normalized) + "\n")
        manifest["artifacts"]["error.normalized.txt"] = sha256_file(
            out / "error.normalized.txt"
        )
        manifest["error_log"] = {
            "raw_sha256": sha256_file(error_log),
            "raw_lines": total,
            "distinct_messages": len(normalized),
        }
    else:
        manifest["warnings"].append("error.log not found")

    # Tier 2 means "needs a running game", which is not the same as "produces a save". The
    # localization runs read their result from the log while the game is still up, so
    # demanding a save would report missing evidence for a run that produced all of it.
    if run["tier"] == 2 and run.get("save_required", True):
        save, superseded = find_save(run_id)
        if superseded:
            manifest["superseded_saves"] = superseded
            print(
                f"  note: {len(superseded)} older save(s) with this run id ignored; "
                f"selected the most recent"
            )
        if save is None:
            manifest["warnings"].append("no save found; tier 2 evidence is missing")
        else:
            if run_id not in save.name:
                manifest["warnings"].append(
                    f"save {save.name} does not contain the run id; verify it is this run's save"
                )
            projection = gamestate.project_save(save)
            (out / "tech-status.json").write_text(
                json.dumps(projection, indent=2, sort_keys=True) + "\n"
            )
            manifest["artifacts"]["tech-status.json"] = sha256_file(out / "tech-status.json")
            manifest["save"] = {
                "name": save.name,
                "relative": str(save.relative_to(SAVES_DIR)),
                "sha256": sha256_file(save),
                "bytes": save.stat().st_size,
            }

    manifest["staging_hashes"] = hash_tree(STAGING)
    (out / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n"
    )

    print(f"captured {run_id} -> {out.relative_to(Path.cwd()) if out.is_relative_to(Path.cwd()) else out}")
    for name in sorted(manifest["artifacts"]):
        print(f"  {name}")
    for warning in manifest["warnings"]:
        print(f"  WARNING: {warning}")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    capture(sys.argv[1])


if __name__ == "__main__":
    main()
