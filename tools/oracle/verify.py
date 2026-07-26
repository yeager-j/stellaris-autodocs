#!/usr/bin/env python3
"""Check that captured oracle records still describe the committed fixtures.

    python3 tools/oracle/verify.py

Every record's manifest names the SHA-256 of each fixture file the run actually loaded.
Editing a fixture after capture — even a comment — breaks the link between the evidence and
the thing that produced it, and the record silently becomes a claim about a file that no
longer exists. This reports that drift instead of letting it pass unnoticed.

Drift is not always wrong. Deliberately changing a fixture means the affected runs must be
re-run, and this names exactly which ones.

Exit status is non-zero when any record drifts, so it can gate a commit.
"""

from __future__ import annotations

import hashlib
import json
import sys

from oracle_paths import MOD_DIR, RECORDS, fixture_files, installed_version


def verify() -> int:
    records = sorted(p for p in RECORDS.iterdir() if (p / "manifest.json").exists())
    if not records:
        print("no captured records")
        return 0

    current = installed_version()
    drifted: list[str] = []
    build_mismatch: list[str] = []

    for record in records:
        manifest = json.loads((record / "manifest.json").read_text())
        problems = []

        for fixture, files in manifest.get("fixture_hashes", {}).items():
            if fixture.startswith("installed:"):
                # An already-installed mod is not staged, so its content lives wherever
                # Steam put it and is not this repository's to pin. Only the descriptor is
                # hashed, which is enough to notice the mod being updated or removed under
                # a record that depended on it.
                descriptor = MOD_DIR / fixture.split(":", 1)[1]
                for _, expected in files.items():
                    if not descriptor.exists():
                        problems.append(f"installed mod gone: {descriptor.name}")
                    elif hashlib.sha256(descriptor.read_bytes()).hexdigest() != expected:
                        problems.append(
                            f"installed mod changed since capture: {descriptor.name}"
                        )
                continue
            try:
                sources = fixture_files(fixture)
            except KeyError:
                problems.append(f"fixture {fixture} no longer exists")
                continue
            for relative, expected in files.items():
                path = sources.get(relative)
                if path is None or not path.exists():
                    problems.append(f"missing {fixture}/{relative}")
                    continue
                actual = hashlib.sha256(path.read_bytes()).hexdigest()
                if actual != expected:
                    problems.append(f"changed {fixture}/{relative}")
            for extra in sorted(set(sources) - set(files)):
                problems.append(f"added since capture {fixture}/{extra}")

        # A record captured against another build documents that build's behavior, not this
        # one. The design requires a changed Stellaris version to invalidate oracle results
        # rather than quietly carry them forward.
        recorded = manifest.get("stellaris", {}).get("version")
        if recorded != current["version"]:
            build_mismatch.append(f"{record.name}: captured on {recorded}")

        status = "DRIFT" if problems else "ok"
        print(f"  {status:5s} {record.name}")
        for problem in problems:
            print(f"          {problem}")
        if problems:
            drifted.append(record.name)

    print()
    if build_mismatch:
        print(f"Installed build is {current['version']}, but:")
        for line in build_mismatch:
            print(f"  {line}")
        print("Re-run those records before trusting them.")
        print()

    if drifted:
        print(f"{len(drifted)} record(s) no longer match the committed fixtures: "
              f"{', '.join(drifted)}")
        print("Re-run them, or restore the fixtures to the state they were captured against.")
        return 1

    print(f"All {len(records)} record(s) match the committed fixtures.")
    return 0


if __name__ == "__main__":
    sys.exit(verify())
