"""Offline tests for scripts/fetch-fixtures.py (`just test` runs them).

Every fetch here is a file:// URL into a temporary directory, so the tests
never touch the network and never write into tests/fixtures/.
"""

from __future__ import annotations

import hashlib
import importlib.util
import io
import re
import tempfile
import tomllib
import unittest
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location(
    "fetch_fixtures", HERE / "fetch-fixtures.py"
)
ff = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ff)


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class Tmp(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def serve(self, name: str, data: bytes) -> str:
        p = self.tmp / "upstream" / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
        return p.as_uri()


class ZipTest(Tmp):
    def test_macos_resource_forks_are_not_corpus_files(self) -> None:
        # Ultimaker 3's 9066.STEP.zip carries `__MACOSX/._9066.STEP`: AppleDouble
        # metadata whose name ends in .STEP but which is not a STEP file.
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w") as z:
            z.writestr("9066.STEP", b"ISO-10303-21;")
            z.writestr("__MACOSX/._9066.STEP", b"\x00\x05\x16\x07")
            z.writestr("sub/._other.stp", b"\x00\x05\x16\x07")
        data = buf.getvalue()
        dest = self.tmp / "out"
        ff.fetch_zip(dest, {"url": self.serve("a.zip", data), "sha256": sha(data)})
        got = sorted(
            p.relative_to(dest).as_posix() for p in dest.rglob("*") if p.is_file()
        )
        self.assertEqual(got, ["9066.STEP"])


class FilesTest(Tmp):
    def test_each_pinned_file_lands_under_its_name(self) -> None:
        a, b = b"ISO-10303-21; a", b"ISO-10303-21; b"
        spec = {
            "files": [
                {
                    "url": self.serve("x/1012-D.STEP", a),
                    "sha256": sha(a),
                    "name": "solidworks/1012-D.STEP",
                },
                {
                    "url": self.serve("y/Mid_Leg.step", b),
                    "sha256": sha(b),
                    "name": "nx/Mid_Leg.step",
                },
            ]
        }
        dest = self.tmp / "out"
        ff.fetch_files(dest, spec)
        self.assertEqual((dest / "solidworks/1012-D.STEP").read_bytes(), a)
        self.assertEqual((dest / "nx/Mid_Leg.step").read_bytes(), b)

    def test_a_changed_upstream_file_is_refused(self) -> None:
        data = b"ISO-10303-21; changed"
        spec = {
            "files": [
                {
                    "url": self.serve("f.step", data),
                    "sha256": "0" * 64,
                    "name": "f.step",
                }
            ]
        }
        with self.assertRaises(SystemExit) as cm:
            ff.fetch_files(self.tmp / "out", spec)
        self.assertIn("sha256 mismatch", str(cm.exception.code))
        self.assertFalse((self.tmp / "out" / "f.step").exists())

    def test_a_present_file_with_the_pinned_hash_is_not_refetched(self) -> None:
        data = b"ISO-10303-21; here"
        dest = self.tmp / "out"
        (dest / "f.step").parent.mkdir(parents=True)
        (dest / "f.step").write_bytes(data)
        # The URL does not exist: a fetch attempt would raise.
        spec = {
            "files": [
                {
                    "url": (self.tmp / "gone.step").as_uri(),
                    "sha256": sha(data),
                    "name": "f.step",
                }
            ]
        }
        ff.fetch_files(dest, spec)
        self.assertEqual((dest / "f.step").read_bytes(), data)

    def test_a_name_cannot_escape_the_source_directory(self) -> None:
        data = b"ISO-10303-21;"
        spec = {
            "files": [
                {
                    "url": self.serve("f.step", data),
                    "sha256": sha(data),
                    "name": "../evil.step",
                }
            ]
        }
        with self.assertRaises(SystemExit):
            ff.fetch_files(self.tmp / "out", spec)
        self.assertFalse((self.tmp / "evil.step").exists())


class EscapeTest(Tmp):
    def test_names_that_leave_the_source_directory_are_refused(self) -> None:
        data = b"ISO-10303-21;"
        url = self.serve("f.step", data)
        for name in (
            "../evil.step",
            str(self.tmp / "abs.step"),
            "",
            ".",
            "a/../../evil.step",
        ):
            with self.subTest(name=name), self.assertRaises(SystemExit):
                ff.fetch_files(
                    self.tmp / "out",
                    {"files": [{"url": url, "sha256": sha(data), "name": name}]},
                )
        self.assertFalse((self.tmp / "evil.step").exists())
        self.assertFalse((self.tmp / "abs.step").exists())

    def test_a_zip_entry_cannot_escape_either(self) -> None:
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w") as z:
            z.writestr("../evil.step", b"ISO-10303-21;")
        data = buf.getvalue()
        with self.assertRaises(SystemExit):
            ff.fetch_zip(
                self.tmp / "out",
                {"url": self.serve("e.zip", data), "sha256": sha(data)},
            )
        self.assertFalse((self.tmp / "evil.step").exists())


class ManifestTest(unittest.TestCase):
    """The committed manifest is the record of what a pass rate means."""

    def setUp(self) -> None:
        self.sources = tomllib.loads(ff.MANIFEST.read_text())["source"]

    def test_every_fetch_kind_is_one_the_fetcher_knows(self) -> None:
        for src in self.sources:
            if "fetch" in src:
                self.assertIn(src["fetch"]["kind"], ff.FETCHERS, src["name"])

    def test_every_source_states_why_and_license(self) -> None:
        for src in self.sources:
            for key in ("why", "license", "expect"):
                self.assertTrue(src.get(key), f"{src['name']} lacks {key}")

    def test_pinned_files_are_immutable_urls(self) -> None:
        # A branch URL can change under the hash; a commit URL cannot.
        for src in self.sources:
            spec = src.get("fetch", {})
            for f in spec.get("files", []) if spec.get("kind") == "files" else []:
                self.assertRegex(f["sha256"], r"^[0-9a-f]{64}$", f["name"])
                self.assertTrue(f.get("writer"), f"{f['name']} lacks writer")
                if "githubusercontent.com" in f["url"]:
                    self.assertRegex(f["url"], r"/[0-9a-f]{40}/", f["name"])
                self.assertFalse(re.search(r"(^|/)\.\.(/|$)", f["name"]), f["name"])


if __name__ == "__main__":
    unittest.main()
