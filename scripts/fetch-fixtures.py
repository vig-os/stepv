#!/usr/bin/env python3
"""Fetch the harness corpus described in tests/fixtures/manifest.toml.

`just fixtures` runs this. Each `[[source]]` with a `fetch` table is fetched
into tests/fixtures/<name>/; sources without one are hand-collected and are
REPORTED as missing rather than skipped in silence (plan.md §7: an empty corpus
must never look green).

Every archive or index is pinned by sha256 in the manifest, and after fetching,
tests/fixtures/corpus.sha256 is rewritten with the hash of every corpus file.
That file is committed: it is the exact byte-level record of what a recorded
pass rate was measured against.

Stdlib only, so it runs from the dev shell's python3 with no environment.
"""

from __future__ import annotations

import gzip
import hashlib
import io
import json
import os
import random
import shutil
import subprocess
import sys
import tomllib
import urllib.request
import zipfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "tests" / "fixtures"
MANIFEST = FIXTURES / "manifest.toml"
LOCK = FIXTURES / "corpus.sha256"
CORPUS_EXTS = {".step", ".stp", ".iges", ".igs", ".brep"}
# Some hosts (nist.gov) refuse python-urllib's default user agent.
UA = {"User-Agent": "stepv-fixtures/1 (+https://github.com/vig-os/stepv)"}


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def get(url: str) -> bytes:
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


def pinned(url: str, want: str) -> bytes:
    data = get(url)
    got = sha256(data)
    if got != want:
        sys.exit(
            f"fixtures: sha256 mismatch for {url}\n  want {want}\n  got  {got}\n"
            "The upstream bytes changed. Re-pin deliberately in manifest.toml."
        )
    return data


def corpus_name(path: Path) -> bool:
    """A corpus file by name: a CAD extension, and not macOS archive metadata.

    Zips made in the Finder carry `__MACOSX/._<name>` AppleDouble entries whose
    names end in the original extension but which hold no STEP at all.
    """
    return (
        path.suffix.lower() in CORPUS_EXTS
        and "__MACOSX" not in path.parts
        and not path.name.startswith("._")
    )


def inside(dest: Path, name: str) -> Path:
    """`dest / name`, refusing any name that resolves outside `dest` (or to it)."""
    out = (dest / name).resolve()
    if out == dest.resolve() or not out.is_relative_to(dest.resolve()):
        sys.exit(f"fixtures: {name!r} escapes {dest}")
    return out


def fetch_zip(dest: Path, spec: dict) -> None:
    data = pinned(spec["url"], spec["sha256"])
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        for info in z.infolist():
            name = Path(info.filename)
            if info.is_dir() or not corpus_name(name):
                continue
            out = inside(dest, Path(*name.parts[spec.get("strip", 0) :]).as_posix())
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_bytes(z.read(info))


def fetch_abc_index(dest: Path, spec: dict, sample: int) -> None:
    base = spec["url"].rsplit("/", 1)[0]
    index = json.loads(pinned(spec["url"], spec["sha256"]))
    # First N by model id: deterministic, and the same slice every run.
    files = sorted(index["files"], key=lambda f: f["id"])[:sample]

    def one(f: dict) -> None:
        out = dest / f["name"]
        if out.exists() and out.stat().st_size == f["stepBytes"]:
            return
        out.write_bytes(gzip.decompress(get(f"{base}/{f['path']}")))

    dest.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(max_workers=8) as pool:
        list(pool.map(one, files))


def fetch_git(dest: Path, spec: dict) -> None:
    """Sparse, blob-filtered checkout of one subdirectory at a pinned commit."""
    tmp = dest.with_name(dest.name + ".git-tmp")
    shutil.rmtree(tmp, ignore_errors=True)
    run = lambda *a: subprocess.run(a, check=True, cwd=tmp, capture_output=True)  # noqa: E731
    tmp.mkdir(parents=True)
    run("git", "init", "-q")
    run("git", "remote", "add", "origin", spec["url"])
    run("git", "sparse-checkout", "set", spec["path"])
    run(
        "git",
        "fetch",
        "-q",
        "--depth",
        "1",
        "--filter=blob:none",
        "origin",
        spec["rev"],
    )
    run("git", "checkout", "-q", "FETCH_HEAD")
    src = tmp / spec["path"]
    for p in src.rglob("*"):
        if p.is_file() and corpus_name(p.relative_to(src)):
            out = dest / p.relative_to(src)
            out.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(p, out)
    shutil.rmtree(tmp)


def fetch_files(dest: Path, spec: dict) -> None:
    """Single files, each pinned by sha256, saved as `name` under `dest`.

    For a corpus drawn one file at a time from many places (the exporter
    matrix): a commit-pinned raw URL per file, and the hash in the manifest.
    """
    for f in spec["files"]:
        out = inside(dest, f["name"])
        if out.is_file() and sha256(out.read_bytes()) == f["sha256"]:
            continue
        data = pinned(f["url"], f["sha256"])
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(data)


def generate_malformed(dest: Path) -> None:
    """Inputs a previewer will be pointed at in the wild. Deterministic."""
    dest.mkdir(parents=True, exist_ok=True)
    # A real, valid file to damage: the smallest NIST part, if fetched.
    donor = next(iter(sorted((FIXTURES / "nist-pmi").rglob("*.stp"))), None)
    real = donor.read_bytes() if donor else b""
    rng = random.Random(0x57E9)
    cases = {
        "empty.step": b"",
        "whitespace.step": b"   \n\n\t\n",
        "not-step-png.step": bytes.fromhex("89504e470d0a1a0a") + rng.randbytes(4096),
        "random-bytes.step": rng.randbytes(65536),
        "header-only.step": (
            b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('header only'),'2;1');\n"
            b"FILE_NAME('x.step','2026-10-04T00:00:00',(''),(''),'','','');\n"
            b"FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\nENDSEC;\n"
            b"END-ISO-10303-21;\n"
        ),
        "not-iges.igs": b"this is not an IGES file\n" * 10,
        "not-brep.brep": b"DBRep_DrawableShape\n\nCASCADE Topology V1, (c) Matra-Datavision\n",
    }
    if real:
        cases["truncated-half.step"] = real[: len(real) // 2]
        cases["truncated-header.step"] = real[:200]
        # Flip bytes inside the DATA section only, so the header still parses.
        data = bytearray(real)
        start = real.find(b"DATA;") + 5
        for _ in range(200):
            i = rng.randrange(start, len(data))
            data[i] = rng.randrange(32, 127)
        cases["bitflipped.step"] = bytes(data)
    for name, body in cases.items():
        (dest / name).write_bytes(body)


def generate_stress(dest: Path, spec: dict) -> None:
    """Many DISTINCT copies of one NIST part in a single STEP file."""
    gen = ROOT / "target" / "kernel" / "stress-gen"
    if not gen.is_file():
        sys.exit(
            "fixtures: stress-assembly needs target/kernel/stress-gen — run `just kernel`"
        )
    donor = FIXTURES / "nist-pmi" / spec["donor"]
    if not donor.is_file():
        sys.exit(
            f"fixtures: stress-assembly donor missing: {donor} (fetch nist-pmi first)"
        )
    out = dest / f"{donor.stem}-x{spec['copies']}.step"
    if out.is_file():
        return
    dest.mkdir(parents=True, exist_ok=True)
    tmp = out.with_suffix(".tmp")
    subprocess.run([str(gen), str(donor), str(spec["copies"]), str(tmp)], check=True)
    tmp.rename(out)


# Every fetch kind the manifest may name: (destination, fetch table, source).
FETCHERS = {
    "zip": lambda dest, spec, src: fetch_zip(dest, spec),
    "abc-index": lambda dest, spec, src: fetch_abc_index(dest, spec, src["sample"]),
    "git": lambda dest, spec, src: fetch_git(dest, spec),
    "files": lambda dest, spec, src: fetch_files(dest, spec),
    "generate": lambda dest, spec, src: generate_malformed(dest),
    "stress": lambda dest, spec, src: generate_stress(dest, spec),
}


def write_lock() -> int:
    lines = []
    for p in sorted(FIXTURES.rglob("*")):
        if p.is_file() and corpus_name(p.relative_to(FIXTURES)):
            lines.append(
                f"{sha256(p.read_bytes())}  {p.relative_to(FIXTURES).as_posix()}"
            )
    LOCK.write_text(
        "# Generated by `just fixtures` — the exact bytes any recorded pass rate\n"
        "# was measured against. Commit it with the numbers it backs.\n"
        + "\n".join(lines)
        + "\n"
    )
    return len(lines)


def main() -> int:
    only = set(sys.argv[1:])
    manifest = tomllib.loads(MANIFEST.read_text())
    missing = []
    for src in manifest["source"]:
        name = src["name"]
        if only and name not in only:
            continue
        dest = FIXTURES / name
        spec = src.get("fetch")
        if spec is None:
            have = dest.is_dir() and any(dest.rglob("*"))
            print(f"  {name:18} hand-collected — {'present' if have else 'MISSING'}")
            if not have:
                missing.append(name)
            continue
        kind = spec["kind"]
        print(f"  {name:18} {kind} ...", flush=True)
        if kind not in FETCHERS:
            sys.exit(f"fixtures: unknown fetch kind {kind!r} for {name}")
        FETCHERS[kind](dest, spec, src)
    n = write_lock()
    print(f"fixtures: {n} corpus files, hashes in {LOCK.relative_to(ROOT)}")
    if missing:
        print(
            f"fixtures: hand-collected sources missing: {', '.join(missing)} — "
            f"drop files into tests/fixtures/<name>/ (see manifest.toml)"
        )
    return 0


if __name__ == "__main__":
    os.umask(0o022)
    sys.exit(main())
