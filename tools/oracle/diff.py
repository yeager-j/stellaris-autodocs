#!/usr/bin/env python3
"""Report the difference between two captured oracle runs.

    python3 tools/oracle/diff.py r0-baseline r1-target

The policy is read from the delta, never from one run's reading. A single observation
cannot separate "the mod's definition won" from "vanilla already behaved this way", and
recording the latter as the former is exactly the assumption the spike exists to remove.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

from oracle_paths import RECORDS

# The three readings that decide the technology row, named so the report states the verdict
# rather than leaving it to be reconstructed from two long lists.
SUBJECTS = [
    ("positive control - must be drawn", "tech_oracle_draw_control"),
    ("SUBJECT - potential omitted", "tech_adaptive_combat_algorithms"),
    ("negative control - must be absent", "tech_neural_implants"),
]


def load(run_id: str) -> dict:
    directory = RECORDS / run_id
    manifest = directory / "manifest.json"
    if not manifest.exists():
        raise SystemExit(f"no capture for {run_id}; run capture.py first")

    data = {"manifest": json.loads(manifest.read_text()), "dir": directory}
    for name, key in (
        ("oracle-facts.txt", "facts"),
        ("error.normalized.txt", "errors"),
    ):
        path = directory / name
        data[key] = path.read_text().splitlines() if path.exists() else []
    tech = directory / "tech-status.json"
    data["tech"] = json.loads(tech.read_text()) if tech.exists() else None
    return data


def section(title: str) -> None:
    print()
    print(title)
    print("-" * len(title))


def report(base_id: str, treat_id: str) -> None:
    base, treat = load(base_id), load(treat_id)

    print(f"{base_id}  ->  {treat_id}")
    print(f"Stellaris {base['manifest']['stellaris']['version']}")

    for run_id, data in ((base_id, base), (treat_id, treat)):
        for warning in data["manifest"].get("warnings", []):
            print(f"  WARNING [{run_id}]: {warning}")

    section("Enabled mods")
    print(f"  {base_id}:  {', '.join(base['manifest']['activation']['enabled_mods'])}")
    print(f"  {treat_id}: {', '.join(treat['manifest']['activation']['enabled_mods'])}")

    section("ORACLE facts")
    base_facts, treat_facts = set(base["facts"]), set(treat["facts"])
    for fact in sorted(treat_facts - base_facts):
        print(f"  + {fact}")
    for fact in sorted(base_facts - treat_facts):
        print(f"  - {fact}")
    if base_facts == treat_facts:
        print("  (identical)")

    if base["tech"] and treat["tech"]:
        section("Research draw pool (alternatives)")
        base_alt = base["tech"].get("alternatives", {})
        treat_alt = treat["tech"].get("alternatives", {})
        for area in sorted(set(base_alt) | set(treat_alt)):
            print(f"  {area}:")
            print(f"    {base_id}:  {', '.join(base_alt.get(area, [])) or '-'}")
            print(f"    {treat_id}: {', '.join(treat_alt.get(area, [])) or '-'}")

        section("Subject membership in the draw pool")
        drawn = {t for techs in treat_alt.values() for t in techs}
        for label, tech in SUBJECTS:
            mark = "DRAWN  " if tech in drawn else "absent "
            print(f"  {mark} {tech:38s} {label}")

        section("Exported potential set")
        base_pot = base["tech"].get("potential", {})
        treat_pot = treat["tech"].get("potential", {})
        gained = sorted(set(treat_pot) - set(base_pot))
        lost = sorted(set(base_pot) - set(treat_pot))
        print(f"  count {len(base_pot)} -> {len(treat_pot)}")
        for tech in gained:
            print(f"  + {tech} = {treat_pot[tech]}")
        for tech in lost:
            print(f"  - {tech} = {base_pot[tech]}")
        if not gained and not lost:
            print("  (no membership change)")

        changed = sorted(
            t for t in set(base_pot) & set(treat_pot) if base_pot[t] != treat_pot[t]
        )
        for tech in changed:
            print(f"  ~ {tech}: {base_pot[tech]} -> {treat_pot[tech]}")
    else:
        section("Exported potential set")
        print("  (not available: one or both runs are tier 1)")

    section("error.log messages")
    base_errors = {line.split("\t", 1)[1]: line.split("\t", 1)[0] for line in base["errors"] if "\t" in line}
    treat_errors = {line.split("\t", 1)[1]: line.split("\t", 1)[0] for line in treat["errors"] if "\t" in line}
    new = sorted(set(treat_errors) - set(base_errors))
    gone = sorted(set(base_errors) - set(treat_errors))
    print(f"  distinct {len(base_errors)} -> {len(treat_errors)}")
    for message in new[:60]:
        print(f"  + [{treat_errors[message]}x] {message}")
    if len(new) > 60:
        print(f"  ... {len(new) - 60} further new messages (see error.normalized.txt)")
    for message in gone[:30]:
        print(f"  - [{base_errors[message]}x] {message}")
    if len(gone) > 30:
        print(f"  ... {len(gone) - 30} further removed messages")


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    report(sys.argv[1], sys.argv[2])


if __name__ == "__main__":
    main()
