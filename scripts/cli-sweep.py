#!/usr/bin/env python3
"""S3 (vig-os/stepv#6): run the real CLI over the whole corpus, default limits.

The harness (examples/harness.rs) judges the KERNEL. This judges the PRODUCT:
the `stepv` binary exactly as a front-end invokes it. It fails (exit 1) when
any run

  - hangs past its own --timeout (plus slack): the limit did not fire;
  - prints anything but one valid JSON line on stdout;
  - exits with a code outside the documented contract (0, 3, 4);
  - exits 3 without its header metadata;
  - reports the kernel crashed.

Usage: scripts/cli-sweep.py [--stepv PATH] [--timeout S] [--size PX] [paths...]
"""

from __future__ import annotations

import argparse
import collections
import json
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXTS = {".step", ".stp", ".iges", ".igs", ".brep"}


def corpus(paths: list[Path]) -> list[Path]:
    out = []
    for p in paths:
        if p.is_dir():
            out += sorted(f for f in p.rglob("*") if f.suffix.lower() in EXTS)
        elif p.suffix.lower() in EXTS:
            out.append(p)
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--stepv", type=Path, default=ROOT / "target/release/stepv")
    ap.add_argument("--timeout", type=float, default=20.0, help="passed to stepv")
    ap.add_argument("--size", type=int, default=256)
    ap.add_argument("paths", nargs="*", type=Path, default=[ROOT / "tests/fixtures"])
    a = ap.parse_args()

    files = corpus(a.paths)
    if not files:
        print("cli-sweep: no corpus files — run `just fixtures`", file=sys.stderr)
        return 1
    problems: list[str] = []
    by_status: collections.Counter[str] = collections.Counter()
    by_code: collections.Counter[int] = collections.Counter()
    slowest: list[tuple[float, str, str]] = []
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "o.png"
        for i, f in enumerate(files, 1):
            t0 = time.monotonic()
            try:
                r = subprocess.run(
                    [
                        str(a.stepv),
                        str(f),
                        "--png",
                        str(out),
                        "--size",
                        str(a.size),
                        "--timeout",
                        str(a.timeout),
                        "--no-cache",
                    ],
                    capture_output=True,
                    timeout=a.timeout + 15,
                )
            except subprocess.TimeoutExpired:
                problems.append(f"HANG (> timeout + 15 s): {f}")
                continue
            wall = time.monotonic() - t0
            lines = r.stdout.decode(errors="replace").splitlines()
            try:
                if len(lines) != 1:
                    raise ValueError(f"{len(lines)} stdout lines")
                j = json.loads(lines[0])
            except ValueError as e:
                problems.append(f"BAD JSON ({e}), exit {r.returncode}: {f}")
                continue
            status = j.get("status", "?")
            by_status[status] += 1
            by_code[r.returncode] += 1
            slowest.append(
                (
                    wall,
                    status,
                    str(f.relative_to(ROOT) if f.is_relative_to(ROOT) else f),
                )
            )
            if r.returncode not in (0, 3, 4):
                problems.append(f"EXIT {r.returncode} outside contract: {f}")
            if r.returncode == 3 and not isinstance(j.get("info"), dict):
                problems.append(f"EXIT 3 without metadata: {f}")
            if status == "crashed":
                problems.append(f"KERNEL CRASH: {f} — {j.get('error')}")
            print(
                f"[{i:>4}/{len(files)}] {r.returncode} {status:<10} {wall * 1e3:>8.0f} ms  {f.name}",
                file=sys.stderr,
            )

    print("\n| Exit code | Files |\n| ---: | ---: |")
    for code, n in sorted(by_code.items()):
        print(f"| {code} | {n} |")
    print("\n| Status | Files |\n| --- | ---: |")
    for st, n in by_status.most_common():
        print(f"| {st} | {n} |")
    print("\n| Slowest | Status | File |\n| ---: | --- | --- |")
    for wall, st, name in sorted(slowest, reverse=True)[:8]:
        print(f"| {wall:.1f} s | {st} | `{name}` |")
    if problems:
        print("\nPROBLEMS:")
        for p in problems:
            print(f"- {p}")
        return 1
    print(f"\ncli-sweep: {len(files)} files, every outcome inside the contract")
    return 0


if __name__ == "__main__":
    sys.exit(main())
