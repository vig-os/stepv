"""ql-e2e's verdict (#64): the installed stepv extension's own log lines for
one request's file, from run-all.sh's `log stream --style ndjson` on stdin
(e2elog.py). The file's path is the request's identity: ql-e2e.sh gives each
request a path of its own, and only lines naming exactly that path count,
whenever they were stamped.

  verdict.py <thumb|preview> <file>

PASS needs at least one line, and every line for the file from the right
extension, running out of /Applications/stepv.app, and saying it drew
("thumbnail <file>: drawn") or showed geometry ("preview <file>: shown"):
one failing line fails the run, whatever else passed. Exit 0 on PASS.

  verdict.py --logged <thumb|preview> <file>

Exit 0 if a result line for the file is in the stream yet, silently: what
ql-e2e.sh waits for before it judges."""

import sys

from e2elog import events

polling = sys.argv[1] == "--logged"
kind, path = sys.argv[1 + polling], sys.argv[2 + polling]
process = {"thumb": "StepvThumbnail", "preview": "StepvPreview"}[kind]
prefix = {"thumb": "thumbnail", "preview": "preview"}[kind] + f" {path}: "
lines = []
requested = False  # the thumbnail extension's "thumbnail request: <file>"
for e in events(sys.stdin):
    m = e["message"]
    requested |= m == f"thumbnail request: {path}"
    if m.startswith(prefix):
        lines.append((e["process"], e["pid"], m[len(prefix) :]))

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
for image, pid, said in lines:
    ours = image.startswith("/Applications/stepv.app/") and image.endswith(
        "/" + process
    )
    good = ours and said.startswith("drawn" if kind == "thumb" else "shown")
    ok &= good
    print(
        f"  {'PASS' if good else 'FAIL'}: {image.rsplit('/', 1)[-1] or '?'} pid {pid}: {said}"
        + ("" if ours else f" (from {image}, not /Applications/stepv.app's {process})")
    )
print("  VERDICT:", "PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
