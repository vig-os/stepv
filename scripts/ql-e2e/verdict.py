"""ql-e2e's verdict (#64): the installed stepv extension's own log line for
one run's file, from `log show --style ndjson` on stdin.

  verdict.py <thumb|preview> <file>

PASS needs at least one line, and every line for the file from the right
extension, running out of /Applications/stepv.app, and saying it drew
("thumbnail <file>: drawn") or showed geometry ("preview <file>: shown"):
one failing line fails the run, whatever else passed. A preview must also
have told Quick Look it was ready (#64, "preview <file>: ready (<why>)"):
a STEP file on its tree, before the scene; IGES and BREP, which have no
tree, on their mesh. Exit 0 on PASS.

  verdict.py --logged <thumb|preview> <file>

Exit 0 if any line for the file is logged yet, silently: what ql-e2e.sh
polls for before it judges."""

import json
import sys

polling = sys.argv[1] == "--logged"
kind, path = sys.argv[1 + polling], sys.argv[2 + polling]
process = {"thumb": "StepvThumbnail", "preview": "StepvPreview"}[kind]
prefix = {"thumb": "thumbnail", "preview": "preview"}[kind] + f" {path}: "
lines = []
readies = []  # (timestamp, why): the preview's handler(nil)
shown_at = ""
requested = False  # the thumbnail extension's "thumbnail request: <file>"
for raw in sys.stdin:
    try:
        e = json.loads(raw)
    except ValueError:
        continue
    m = e.get("eventMessage", "")
    requested |= m == f"thumbnail request: {path}"
    if m.startswith(prefix + "ready ("):
        readies.append((e.get("timestamp", ""), m[len(prefix) + 7 :].split(")")[0]))
    elif m.startswith(prefix):
        lines.append((e.get("processImagePath", ""), m[len(prefix) :]))
        shown_at = shown_at or e.get("timestamp", "")

if polling:
    sys.exit(0 if lines else 1)
ok = bool(lines)
if not lines and kind == "thumb" and not requested:
    # Quick Look's thumbnail cache is keyed on the file's inode: it answers
    # a file it has seen without calling the extension (ql-e2e.sh writes a
    # fresh file per request so that never happens; this catches it if it
    # does).
    print(
        f"  no request for {path}: QL served a cached thumbnail; the extension did not run"
    )
elif not lines and requested:
    print(f"  {path}: the extension got the request but logged no result")
elif not lines:
    print(f"  no log line for {path}: the extension did not run, or not this build")
for image, said in lines:
    ours = image.startswith("/Applications/stepv.app/") and image.endswith(
        "/" + process
    )
    good = ours and said.startswith("drawn" if kind == "thumb" else "shown")
    ok &= good
    print(
        f"  {'PASS' if good else 'FAIL'}: {image.rsplit('/', 1)[-1] or '?'}: {said}"
        + ("" if ours else f" (from {image}, not /Applications/stepv.app's {process})")
    )
if kind == "preview" and lines:
    want = "tree" if path.lower().endswith((".step", ".stp")) else "mesh"
    first = readies[0] if readies else ("", "never")
    good = first[1] == want and (want != "tree" or first[0] < shown_at)
    ok &= good and len(readies) == 1
    order = ", before the scene" if want == "tree" else ""
    print(
        f"  {'PASS' if good and len(readies) == 1 else 'FAIL'}: ready ({first[1]}),"
        f" {len(readies)} time(s); wanted once, on its {want}{order}"
    )
print("  VERDICT:", "PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
