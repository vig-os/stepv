"""ql-e2e's checks on one request's run in the supervisor service (#64),
from run-all.sh's log stream on stdin (e2elog.py). Each request is its file:
the extension's "accepted" line names the run and the service that took it,
and only that run's own lines count. What came first is the stream's order,
never a timestamp (#111).

  runs.py spawned <kind> <file>      its kernel pid, once spawned (exit 1 before)
  runs.py limit <kind> <file>        the admission limit of the service that took it
  runs.py concurrent <held> <beside> (thumbnail) <beside> was never queued, and its
                                     kernel exited before <held>'s was killed
  runs.py queued <held> <beside>     (thumbnail) <beside> was queued behind <held>,
                                     and spawned only after <held>'s kill, if at all
  runs.py deadline <file>            (thumbnail) killed at its deadline; its kernel pid
  runs.py cancelled <file> <from> <to>
                                     (preview) cancelled with SIGKILL after the host's
                                     line <from> and before its line <to>; its kernel pid
  runs.py logged <message>          whether any line says exactly <message>
  runs.py reused <file>              (preview) whether a controller was prepared again
                                     for another file after <file>

Each prints its finding; exit 0 on ok."""

import re
import sys

from e2elog import events, run_lines, run_of

evs = list(events(sys.stdin))
what, args = sys.argv[1], sys.argv[2:]


def lines_of(kind, path):
    r = run_of(evs, kind, path)
    if r is None:
        sys.exit(print(f"no run accepted for {kind} {path}") or 1)
    return r, run_lines(evs, *r)


def first(lines, pattern):
    """(index, match) of the first of `lines` matching `pattern`, or None."""
    for i, m in lines:
        found = re.search(pattern, m)
        if found:
            return i, found
    return None


SPAWNED = r"kernel pid (\d+) spawned"
ENDED = r"exit -?\d+ in \d+ ms|kernel pid \d+ killed \(|cancelled, kernel pid"

if what == "spawned":
    _, lines = lines_of(*args)
    s = first(lines, SPAWNED)
    sys.exit(print(s[1].group(1)) if s else 1)

if what == "limit":
    (_, pid), _ = lines_of(*args)
    for e in evs:
        m = re.match(
            rf"service pid {pid} started: .*admission limit (\d+)", e["message"]
        )
        if e["category"] == "supervisor" and m:
            sys.exit(print(m.group(1)))
    sys.exit(print(f"service pid {pid} logged no start line") or 1)

if what in ("concurrent", "queued"):
    (_, hpid), held = lines_of("thumbnail", args[0])
    (_, bpid), beside = lines_of("thumbnail", args[1])
    kill = first(held, r"kernel pid \d+ killed \(deadline\)")
    queued = first(beside, r"\): queued, ")
    spawn = first(beside, SPAWNED)
    if hpid != bpid:
        sys.exit(
            print(
                f"two services: pid {hpid} took the held run, pid {bpid} the one beside"
            )
            or 1
        )
    if not kill:
        sys.exit(print("the held run was not killed at its deadline") or 1)
    if what == "concurrent":
        end = first(beside, ENDED)
        ok = not queued and spawn and end and end[0] < kill[0]
        print(
            "the run beside was "
            + ("queued" if queued else "never queued")
            + (
                ", its kernel ended before the held one's kill"
                if end and end[0] < kill[0]
                else ", and did not end before the held one's kill"
            )
        )
    else:
        never = first(beside, r"never spawned")
        ok = queued and ((spawn and spawn[0] > kill[0]) or (not spawn and never))
        print(
            "the run beside was "
            + ("queued" if queued else "not queued")
            + (
                ", spawned after the held one's kill"
                if spawn and spawn[0] > kill[0]
                else ", spawned BEFORE the held one's kill"
                if spawn
                else ", never spawned"
                if never
                else ", never seen to end"
            )
        )
    sys.exit(0 if ok else 1)

if what == "deadline":
    _, lines = lines_of("thumbnail", args[0])
    k = first(
        lines,
        r"kernel pid (\d+) killed \(deadline\) at (\d+) ms, reaped in ([\d.]+) ms",
    )
    if not k:
        sys.exit(print("no deadline kill logged for its run") or 1)
    sys.exit(
        print(
            k[1].group(1), f"killed at {k[1].group(2)} ms, reaped in {k[1].group(3)} ms"
        )
    )

if what == "cancelled":
    path, start, end = args
    _, lines = lines_of("preview", path)
    host = {e["message"]: i for i, e in enumerate(evs) if e["category"] == "ql-e2e"}
    if start not in host or end not in host:
        sys.exit(
            print(f"the host never logged '{start if start not in host else end}'") or 1
        )
    c = first(
        lines,
        r"cancelled, kernel pid (\d+) signal 9, reaped ([\d.]+) ms after the cancel",
    )
    if not c:
        sys.exit(print("its run was never cancelled") or 1)
    ok = host[start] < c[0] < host[end] and float(c[1].group(2)) <= 1000
    said = start.split()[2]  # "closed" or "switched"
    where = (
        f"before the host {said} it"
        if c[0] < host[start]
        else f"after the host {said} it, before it quit"
        if c[0] < host[end]
        else "only after the host quit"
    )
    sys.exit(
        print(
            c[1].group(1),
            f"cancelled {where}, reaped {c[1].group(2)} ms after the cancel",
        )
        or (0 if ok else 1)
    )

if what == "logged":
    sys.exit(0 if any(e["message"] == args[0] for e in evs) else 1)

if what == "reused":
    said = [
        e["message"]
        for e in evs
        if e["message"].startswith(f"preview controller reused: {args[0]} ")
    ]
    sys.exit(
        print(said[0] if said else "no controller was prepared twice")
        or (0 if said else 1)
    )

sys.exit(print(__doc__) or 2)
