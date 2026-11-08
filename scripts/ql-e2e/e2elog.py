"""ql-e2e's view of the unified log (#64): the events of one `log stream
--style ndjson`, started before the first request, read from its file.

Events are told apart by who they are (the request's file path, the
service pid, the run number), never by when they were logged. A log
event's timestamp is not on the clock the scripts read: on CI's macOS VM it
ran ~0.7 s behind `date` (#113's CI: the extension's "thumbnail <file>:
drawn" line, for a file written after the request's start was taken, was
stamped before that start). Every window opened at a wall-clock instant,
whole-second or to the microsecond, dropped a request's own line, and the
verdict called a 769 ms thumbnail cached. So no time is compared here:
what came first is the order of the stream.

A request's kernel is found the same way. The extension logs which run of
which service took its file ("accepted: run #N (service pid P), <kind>
<path>", StepvKernel.logAccepted), and the service's own lines for that run
begin "run #N (service pid P" (macos/Supervisor/main.swift)."""

import json
import re


def events(stream):
    """The events of `stream` (ndjson), each a dict with "message",
    "category", "process" (image path) and "pid" (int, or None)."""
    for raw in stream:
        try:
            e = json.loads(raw)
        except ValueError:
            continue  # log stream's header line
        if not isinstance(e, dict) or "eventMessage" not in e:
            continue
        yield {
            "message": e["eventMessage"],
            "category": e.get("category", ""),
            "process": e.get("processImagePath", ""),
            "pid": e.get("processID"),
        }


ACCEPTED = re.compile(
    r"accepted: run #(\d+) \(service pid (\d+)\), (thumbnail|preview) (.+)"
)


def run_of(evs, kind, path):
    """(run, service pid) that took `path`'s `kind` ("thumbnail" or
    "preview") request, from the extension's own line; None if not (yet)."""
    for e in evs:
        m = ACCEPTED.fullmatch(e["message"])
        if (
            e["category"] == "quicklook"
            and m
            and m.group(3) == kind
            and m.group(4) == path
        ):
            return int(m.group(1)), int(m.group(2))
    return None


def run_lines(evs, run, pid):
    """(stream index, message) of each line the service logged for that run
    of service pid `pid`: its alone, the pid anchored ("pid 45" is never
    "pid 451"'s)."""
    own = re.compile(rf"run #{run} \(service pid {pid}[,)]")
    return [
        (i, e["message"])
        for i, e in enumerate(evs)
        if e["category"] == "supervisor" and own.match(e["message"])
    ]
