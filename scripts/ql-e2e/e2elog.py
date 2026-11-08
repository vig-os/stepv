"""ql-e2e's view of the unified log (#64): the events of one `log stream
--style ndjson`, started before the first request, read from its file.

Events are told apart by who they are (the request's file path, the
service pid, the run number), never by when they were logged. A log
event's timestamp is not on the clock the scripts read: on CI's macOS VM it
ran ~0.7 s behind `date` (#113's CI: the extension's "thumbnail <file>:
drawn" line, for a file written after the request's start was taken, was
stamped before that start). Every window opened at a wall-clock instant,
whole-second or to the microsecond, dropped a request's own line, and the
verdict called a 769 ms thumbnail cached. So no time is compared here."""

import json


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
