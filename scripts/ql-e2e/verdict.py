"""ql-e2e's verdict (#64): the installed stepv extension's own log line for
one run's file, from `log show --style ndjson` on stdin.

  verdict.py <thumb|preview> <file>

PASS needs the line from the right extension, running out of
/Applications/stepv.app, and saying it drew ("thumbnail <file>: drawn") or
showed geometry ("preview <file>: shown"). Exit 0 on PASS."""

import json
import sys

kind, path = sys.argv[1], sys.argv[2]
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

ok = False
if not lines:
    print(f"  no log line for {path}: the extension did not run, or not this build")
for image, said in lines:
    ours = image.startswith("/Applications/stepv.app/") and image.endswith(
        "/" + process
    )
    good = ours and said.startswith("drawn" if kind == "thumb" else "shown")
    ok |= good
    print(
        f"  {'PASS' if good else 'FAIL'}: {image.rsplit('/', 1)[-1] or '?'}: {said}"
        + ("" if ours else f" (from {image}, not /Applications/stepv.app's {process})")
    )
print("  VERDICT:", "PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
