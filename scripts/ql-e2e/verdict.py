"""ql-e2e's verdict (#64): the installed stepv extension's own log line for
one run's file, from `log show --style ndjson` on stdin.

  verdict.py <thumb|preview> <file>

PASS needs at least one line, and every line for the file from the right
extension, running out of /Applications/stepv.app, and saying it drew
("thumbnail <file>: drawn") or showed geometry ("preview <file>: shown"):
one failing line fails the run, whatever else passed. Exit 0 on PASS.

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
for raw in sys.stdin:
    try:
        e = json.loads(raw)
    except ValueError:
        continue
    m = e.get("eventMessage", "")
    if m.startswith(prefix):
        lines.append((e.get("processImagePath", ""), m[len(prefix) :]))

if polling:
    sys.exit(0 if lines else 1)
ok = bool(lines)
if not lines:
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
print("  VERDICT:", "PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
