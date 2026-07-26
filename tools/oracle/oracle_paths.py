"""Shared locations and hashing for the resolver oracle harness.

Every path is overridable by environment variable so the harness can run against another
installation without editing code. The defaults are the macOS Steam layout recorded in
AGENTS.md.
"""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
FIXTURES = REPO_ROOT / "fixtures" / "oracle"
RECORDS = REPO_ROOT / "docs" / "spikes" / "oracle-records"
RUNS_FILE = Path(__file__).resolve().parent / "runs.json"


def _env_path(name: str, default: str) -> Path:
    return Path(os.environ.get(name, default)).expanduser()


INSTALL_ROOT = _env_path(
    "STELLARIS_INSTALL_ROOT",
    "~/Library/Application Support/Steam/steamapps/common/Stellaris",
)
DATA_ROOT = _env_path(
    "STELLARIS_DATA_ROOT",
    "~/Documents/Paradox Interactive/Stellaris",
)

MOD_DIR = DATA_ROOT / "mod"
LOGS_DIR = DATA_ROOT / "logs"
SAVES_DIR = DATA_ROOT / "save games"
DLC_LOAD = DATA_ROOT / "dlc_load.json"
EXECUTABLE = INSTALL_ROOT / "stellaris.app" / "Contents" / "MacOS" / "stellaris"
LAUNCHER_SETTINGS = INSTALL_ROOT / "launcher-settings.json"

# Everything the harness materializes lives under one directory so restore is a single
# delete and cannot strand fixture mods among the user's real ones.
STAGING = MOD_DIR / "oracle"
DESCRIPTOR_PREFIX = "oracle_"
BACKUP = DATA_ROOT / "dlc_load.json.oracle-backup"


def load_runs() -> dict:
    return json.loads(RUNS_FILE.read_text())


def fixture_files(name: str) -> dict[str, Path]:
    """Logical path -> source file for one fixture, with any variant derivation applied.

    Variants carry no content. `target-replace-path` and `target-reordered` are derivations
    of `target`, so the content has one authority and the runs cannot drift apart while
    appearing to test the same thing.

    This is that authority. Activation copies what it returns and verification hashes what
    it returns, so a variant cannot mean one thing when it is staged for the game and
    another when its captured record is checked.
    """
    source = FIXTURES / name
    if not source.is_dir():
        raise KeyError(f"unknown fixture: {name}")

    variant_file = source / "variant.json"
    if not variant_file.exists():
        return {
            p.relative_to(source).as_posix(): p
            for p in source.rglob("*")
            if p.is_file()
        }

    variant = json.loads(variant_file.read_text())
    base = FIXTURES / variant["base"]
    rename = variant.get("rename", {})

    files = {
        rename.get(rel, rel): path
        for path in base.rglob("*")
        if path.is_file()
        for rel in [path.relative_to(base).as_posix()]
    }
    if variant.get("descriptor"):
        files["descriptor.mod"] = source / variant["descriptor"]
    return files


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def hash_tree(root: Path) -> dict[str, str]:
    """SHA-256 per file, keyed by POSIX-normalized path relative to root.

    Ordering follows the technical design's canonical rule: sort by the normalized path
    bytes, not by filesystem enumeration order, so the result does not depend on how the
    directory happened to be walked.
    """
    entries = {}
    for path in sorted(root.rglob("*"), key=lambda p: str(p.relative_to(root)).encode()):
        if path.is_file():
            entries[path.relative_to(root).as_posix()] = sha256_file(path)
    return entries


def installed_version() -> dict:
    """The installed build, from the launcher settings rather than from a log.

    game.log's banner records the last run, not what is installed, so the two disagree
    whenever logs are stale. Callers that want the banner should read it separately and
    record both.
    """
    settings = json.loads(LAUNCHER_SETTINGS.read_text())
    return {
        "version": settings.get("version"),
        "rawVersion": settings.get("rawVersion"),
        "modsCompatibilityVersion": settings.get("modsCompatibilityVersion"),
        "distPlatform": settings.get("distPlatform"),
    }
