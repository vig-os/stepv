"""runs.py's own check (#64), on made-up streams: each verdict on a stream
that should pass it, and on one that should fail it, among them a decoy
whose service pid has the real one's as a prefix (45210, 4521) and whose
run has the same number. Run by run-all.sh before Quick Look is asked
anything. Exit 0 if every case comes out as it should."""

import json
import os
import subprocess
import sys

here = os.path.dirname(os.path.abspath(__file__))


def ev(cat, msg):
    return json.dumps(
        {"eventMessage": msg, "category": cat, "processImagePath": "/x", "processID": 1}
    )


def run(stream, *args):
    p = subprocess.run(
        [sys.executable, os.path.join(here, "runs.py"), *args],
        input="\n".join(stream) + "\n",
        capture_output=True,
        text=True,
    )
    return p.returncode, p.stdout.strip()


Q, S, H = "quicklook", "supervisor", "ql-e2e"
start = [
    ev(
        S,
        "service pid 4521 started: swept 0 dead run(s), kept 0 live, admission limit 2, x",
    ),
    ev(
        S,
        "service pid 45210 started: swept 0 dead run(s), kept 0 live, admission limit 1, x",
    ),
]
held = [
    ev(Q, "accepted: run #1 (service pid 4521), thumbnail /h.step"),
    ev(S, "run #1 (service pid 4521, up 1 ms): kernel pid 700 spawned in 0.3 ms"),
]
decoy = [
    ev(S, "run #2 (service pid 45210): queued, 1 kernel(s) running"),
    ev(S, "run #2 (service pid 45210, up 9 ms): kernel pid 999 spawned in 0.3 ms"),
    ev(S, "run #2 (service pid 45210, up 9 ms): exit 0 in 50 ms"),
]
beside = [
    ev(Q, "accepted: run #2 (service pid 4521), thumbnail /b.brep"),
    ev(S, "run #2 (service pid 4521, up 2 ms): kernel pid 701 spawned in 0.3 ms"),
    ev(S, "run #2 (service pid 4521, up 2 ms): exit 0 in 90 ms"),
]
kill = [
    ev(
        S,
        "run #1 (service pid 4521, up 1 ms): kernel pid 700 killed (deadline) at 15020 ms, reaped in 40.1 ms",
    )
]
queued = [
    ev(Q, "accepted: run #2 (service pid 4521), thumbnail /b.brep"),
    ev(S, "run #2 (service pid 4521): queued, 1 kernel(s) running"),
]
after = [ev(S, "run #2 (service pid 4521, up 2 ms): kernel pid 701 spawned in 0.3 ms")]
prev = [
    ev(Q, "accepted: run #3 (service pid 4521), preview /p.step"),
    ev(S, "run #3 (service pid 4521, up 1 ms): kernel pid 702 spawned in 0.3 ms"),
]
cancel = [
    ev(
        S,
        "run #3 (service pid 4521, up 1 ms): cancelled, kernel pid 702 signal 9, reaped 20.5 ms after the cancel",
    )
]
closed, quits = (
    [ev(H, "preview host closed /p.step")],
    [ev(H, "preview host quits /p.step")],
)
c = (
    "cancelled",
    "/p.step",
    "preview host closed /p.step",
    "preview host quits /p.step",
)

cases = [
    ("spawned", start + held + decoy, ("spawned", "thumbnail", "/h.step"), 0),
    ("spawned: not the decoy's", start + decoy, ("spawned", "thumbnail", "/b.brep"), 1),
    ("limit: its own service's", start + held, ("limit", "thumbnail", "/h.step"), 0),
    (
        "concurrent",
        start + held + decoy + beside + kill,
        ("concurrent", "/h.step", "/b.brep"),
        0,
    ),
    (
        "concurrent: ended after the kill",
        start + held + kill + beside,
        ("concurrent", "/h.step", "/b.brep"),
        1,
    ),
    (
        "concurrent: the decoy's end is not its",
        start + held + queued + decoy + kill + after,
        ("concurrent", "/h.step", "/b.brep"),
        1,
    ),
    (
        "queued",
        start + held + queued + decoy + kill + after,
        ("queued", "/h.step", "/b.brep"),
        0,
    ),
    (
        "queued: spawned before the kill",
        start + held + queued + after + kill,
        ("queued", "/h.step", "/b.brep"),
        1,
    ),
    (
        "queued: never queued",
        start + held + beside + kill,
        ("queued", "/h.step", "/b.brep"),
        1,
    ),
    ("deadline", start + held + kill, ("deadline", "/h.step"), 0),
    ("deadline: none", start + held, ("deadline", "/h.step"), 1),
    ("cancelled between", start + prev + closed + cancel + quits, c, 0),
    ("cancelled: only at the quit", start + prev + closed + quits + cancel, c, 1),
    ("cancelled: never", start + prev + closed + quits, c, 1),
    ("cancelled: no close logged", start + prev + cancel + quits, c, 1),
]
pr = [ev(Q, "preview /p.step: ready (tree) in 90 ms")]
shown = [ev(Q, "preview /p.step: shown: STEP; 4000 parts")]
stale = [ev(Q, "stale, dropped: preview /p.step result")]
cases += [
    (
        "gone",
        start + prev + pr + closed + cancel + stale + quits,
        ("gone", "/p.step"),
        0,
    ),
    (
        "gone: a result shown",
        start + prev + pr + closed + shown + quits,
        ("gone", "/p.step"),
        1,
    ),
    (
        "gone: completed twice",
        start + prev + pr + closed + pr + quits,
        ("gone", "/p.step"),
        1,
    ),
    ("gone: never completed", start + prev + closed + quits, ("gone", "/p.step"), 1),
]
cases += [
    ("first", start + prev + pr + pr, ("first", "preview /p.step: ready ("), 0),
    ("first: none", start + prev, ("first", "preview /p.step: ready ("), 1),
]
o = ("order", "preview host closed /p.step", "preview /p.step: ready (tree) in 90 ms")
cases += [
    ("order", start + closed + pr + quits, o, 0),
    ("order: after", start + pr + closed + quits, o, 1),
    ("order: b never", start + closed, o, 0),
    ("order: a never", start + pr, o, 1),
]
cases += [
    (
        "cancelled after its ready",
        start + prev + closed + pr + cancel + quits,
        (
            "cancelled",
            "/p.step",
            "preview /p.step: ready (tree) in 90 ms",
            "preview host quits /p.step",
        ),
        0,
    ),
    (
        "cancelled: before its ready",
        start + prev + closed + cancel + pr + quits,
        (
            "cancelled",
            "/p.step",
            "preview /p.step: ready (tree) in 90 ms",
            "preview host quits /p.step",
        ),
        1,
    ),
]
failed = 0
for name, stream, args, want in cases:
    rc, out = run(stream, *args)
    ok = (rc == 0) == (want == 0)
    failed += not ok
    print(f"{'ok' if ok else 'FAIL'}: runs.py {name}: exit {rc} ({out})")
if run(start + held, "spawned", "thumbnail", "/h.step")[1] != "700":
    failed += 1
    print("FAIL: runs.py spawned: not kernel pid 700")
sys.exit(1 if failed else 0)
