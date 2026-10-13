"""ql-e2e's view of the unified log (#64): `log show --style ndjson` events,
kept only from a request's own start on, to the microsecond.

`log show --start` takes whole seconds, so a window opened by it holds the
previous request's lines too when both fall in one second; matched by file
alone, a cached thumbnail then passed on the line of the request before it
(#107's CI). Every ql-e2e assertion goes through `events`, with the
request's start from `now()`, taken just before the request is made."""

import json
from datetime import datetime


def now():
    """This instant, as `events` takes it: local, with the zone."""
    return datetime.now().astimezone().isoformat()


def events(stream, since):
    """The events of `stream` (ndjson) stamped at or after `since` (ISO),
    each a dict with "message", "process" (image path), "time" (datetime)."""
    start = datetime.fromisoformat(since)
    for raw in stream:
        try:
            e = json.loads(raw)
            t = datetime.strptime(e["timestamp"], "%Y-%m-%d %H:%M:%S.%f%z")
        except (ValueError, KeyError):
            continue  # not an event (log show's closing summary has no time)
        if t >= start:
            yield {
                "message": e.get("eventMessage", ""),
                "process": e.get("processImagePath", ""),
                "time": t,
            }


def attributed(stream, since):
    """(time, service, message) for each event from `since` on. A
    supervisor line names its service by pid ("service pid N"), and each
    service names itself in its start line ("..., ch.exoma.stepv.thumbnail.
    supervisor"): the thumbnail's and the preview's supervisors are one
    binary, so the image path cannot tell them apart. Start lines before
    `since` count too, for services started earlier: pass the whole run's
    events. Service: "thumbnail", "preview", or "" for other lines."""
    import re

    service, start = {}, datetime.fromisoformat(since)
    for e in events(stream, "1970-01-01T00:00:00+00:00"):
        m = e["message"]
        s = re.search(
            r"service pid (\d+) started.*ch\.exoma\.stepv\.(thumbnail|preview)\.supervisor",
            m,
        )
        if s:
            service[s.group(1)] = s.group(2)
        if e["time"] < start:
            continue
        p = re.search(r"service pid (\d+)", m)
        yield e["time"], service.get(p.group(1), "?") if p else "", m


if __name__ == "__main__":
    # e2elog.py <since>: ndjson on stdin, "time<TAB>service<TAB>message" out.
    import sys

    for t, svc, m in attributed(sys.stdin, sys.argv[1]):
        print(f"{t.isoformat()}\t{svc}\t{m}")
