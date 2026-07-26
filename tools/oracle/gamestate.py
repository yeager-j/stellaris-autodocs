"""Project the player country's technology state out of a Stellaris save.

A `.sav` is a zip holding `gamestate` in Clausewitz text, tens of megabytes uncompressed
and derived from proprietary content. Only the projection is kept: the exported technology
sets for the player country, which is the whole of what the oracle reads.

`country.<id>.tech_status.potential` is the game's own computed set of currently drawable
technologies. It is exported engine state rather than a rendered tooltip, which is why it
is the primary observation for effective technology eligibility.
"""

from __future__ import annotations

import re
import zipfile
from pathlib import Path

PLAYER_COUNTRY = re.compile(r"^\s*country=(\d+)\s*$")
QUOTED = re.compile(r'"([^"]+)"(?:="([^"]*)")?')


def _depth_delta(line: str) -> int:
    return line.count("{") - line.count("}")


def _read_block(lines: list[str], start: int) -> tuple[list[str], int]:
    """Return the body of the block whose `key=` line is at `start`, and the index after it.

    The opening brace may share the key's line or sit on the next one; both forms occur in
    gamestate.
    """
    index = start
    depth = 0
    opened = False
    body: list[str] = []

    while index < len(lines):
        line = lines[index]
        delta = _depth_delta(line)
        if not opened:
            if "{" in line:
                opened = True
                depth = delta
                if index != start:
                    body.append(line)
            index += 1
            continue
        depth += delta
        if depth <= 0:
            return body, index + 1
        body.append(line)
        index += 1

    return body, index


def _find_key(
    lines: list[str], key: str, start: int = 0, top_level: bool = False
) -> int | None:
    """Index of the first line declaring `key`.

    `top_level` requires column zero. Without it, a search for `country` matches the
    `country=0` reference inside the `player` block long before the top-level `country`
    collection, and every lookup beneath it reads the wrong subtree while still returning
    plausible-looking structure.
    """
    wanted = key + "="
    for index in range(start, len(lines)):
        line = lines[index]
        if top_level:
            if line.startswith(wanted):
                return index
        elif line.strip().startswith(wanted):
            return index
    return None


def player_country_id(lines: list[str]) -> int | None:
    index = _find_key(lines, "player", top_level=True)
    if index is None:
        return None
    body, _ = _read_block(lines, index)
    for line in body:
        match = PLAYER_COUNTRY.match(line)
        if match:
            return int(match.group(1))
    return None


def _country_body(lines: list[str], country_id: int) -> list[str] | None:
    index = _find_key(lines, "country", top_level=True)
    if index is None:
        return None
    countries, _ = _read_block(lines, index)
    entry = _find_key(countries, str(country_id))
    if entry is None:
        return None
    body, _ = _read_block(countries, entry)
    return body


def project(gamestate_lines: list[str]) -> dict:
    """Extract the player country's potential and alternatives sets.

    Returns sorted collections. Save order reflects internal iteration and is not a fact
    about resolution, so preserving it would make two equivalent runs look different.
    """
    country_id = player_country_id(gamestate_lines)
    if country_id is None:
        return {"error": "no player country found in gamestate"}

    body = _country_body(gamestate_lines, country_id)
    if body is None:
        return {"error": f"country {country_id} not found in gamestate"}

    tech_index = _find_key(body, "tech_status")
    if tech_index is None:
        return {"error": f"country {country_id} has no tech_status"}
    tech_status, _ = _read_block(body, tech_index)

    result: dict = {"player_country": country_id}

    potential_index = _find_key(tech_status, "potential")
    if potential_index is not None:
        potential_body, _ = _read_block(tech_status, potential_index)
        potential = {}
        for line in potential_body:
            match = QUOTED.search(line)
            if match:
                potential[match.group(1)] = match.group(2)
        result["potential"] = dict(sorted(potential.items()))
        result["potential_count"] = len(potential)

    alternatives_index = _find_key(tech_status, "alternatives")
    if alternatives_index is not None:
        alternatives_body, _ = _read_block(tech_status, alternatives_index)
        alternatives: dict[str, list[str]] = {}
        current = None
        for line in alternatives_body:
            stripped = line.strip()
            if stripped.endswith("=") and not stripped.startswith('"'):
                current = stripped[:-1]
                alternatives[current] = []
            elif stripped.startswith('"') and current:
                alternatives[current].append(stripped.strip('"'))
        result["alternatives"] = {k: sorted(v) for k, v in sorted(alternatives.items())}

    researched = sorted(
        {
            match.group(1)
            for line in tech_status
            if line.strip().startswith("technology=")
            for match in [QUOTED.search(line)]
            if match
        }
    )
    result["researched"] = researched
    result["researched_count"] = len(researched)

    return result


def project_save(save_path: Path) -> dict:
    with zipfile.ZipFile(save_path) as archive:
        raw = archive.read("gamestate").decode("utf-8", errors="replace")
    return project(raw.splitlines())
