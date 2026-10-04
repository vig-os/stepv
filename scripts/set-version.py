#!/usr/bin/env python3
"""Set the stepv version everywhere it lives: both crate manifests and the
lockfile entries for them. Run by the prepare-release extension on the
release branch (devkit bumps CHANGELOG.md only); runnable by hand.

Usage: scripts/set-version.py X.Y.Z[-rcN]
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SEMVER = re.compile(r"^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$")
CRATES = {"stepv": ROOT / "Cargo.toml", "stepv-capi": ROOT / "capi" / "Cargo.toml"}


def set_manifest(path: Path, version: str) -> None:
    text = path.read_text()
    # Only the [package] table's version: the first `version =` after it.
    head, sep, rest = text.partition("[package]")
    if not sep:
        sys.exit(f"set-version: no [package] in {path}")
    new_rest, n = re.subn(
        r'(?m)^version = "[^"]*"', f'version = "{version}"', rest, count=1
    )
    if n != 1:
        sys.exit(f"set-version: no package version in {path}")
    path.write_text(head + sep + new_rest)


def set_lock(path: Path, version: str) -> None:
    text = path.read_text()
    for name in CRATES:
        pattern = rf'(\[\[package\]\]\nname = "{re.escape(name)}"\nversion = )"[^"]*"'
        text, n = re.subn(pattern, rf'\g<1>"{version}"', text)
        if n != 1:
            sys.exit(f"set-version: {name} not found exactly once in {path}")
    path.write_text(text)


def main() -> int:
    if len(sys.argv) != 2 or not SEMVER.match(sys.argv[1]):
        sys.exit("usage: set-version.py X.Y.Z[-rcN]")
    version = sys.argv[1]
    for path in CRATES.values():
        set_manifest(path, version)
    set_lock(ROOT / "Cargo.lock", version)
    print(
        f"set-version: {version} in {', '.join(str(p.relative_to(ROOT)) for p in CRATES.values())}, Cargo.lock"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
