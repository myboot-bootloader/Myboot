"""Parse MyBoot's serial console output into a list of typed events.

ONE job: text in, ``events`` out. Pure and dependency-free, so it is unit-tested
without QEMU. The regexes match the exact lines MyBoot logs from its pipeline
(``crates/engine/src/pipeline.rs``) and entry point (``bin/myboot/src/main.rs``).
Lines that match nothing (firmware chatter, the UEFI shell banner) are ignored.
"""
from __future__ import annotations

import re
from typing import List

from . import events as ev

# Each pattern captures the fields of one event kind. Anchored on the "myboot:"
# marker so unrelated firmware output is skipped.
_SCAN = re.compile(r"myboot: scanning (\d+) volume")
_FSPROBE = re.compile(r"myboot: fs probe (.+?) exists=(true|false)")
_DISCOVERED = re.compile(r"myboot: discovered (\d+) entr\S* (\d+) bootable")
_ENTRY = re.compile(
    r"myboot:\s+'(.*?)' loader=(None|Some\(\"(.*?)\"\)) health=(\w+)"
)
_HANDOFF = re.compile(r"myboot: handing off to '(.+?)'")
_HANDOFF_FAILED = re.compile(
    r"myboot: handoff to '(.+?)' failed \((.+?)\); trying next"
)
_NO_PLAN = re.compile(r"myboot: no launch plan for '(.+?)'")
_NO_CAND = re.compile(r"myboot: no candidate could be booted \((\d+) tried\)")
_GOP = re.compile(r"myboot: GOP unavailable \((.+?)\)")
_ENGINE_ERR = re.compile(r"myboot: engine could not hand off: (.+?)\s*$")
_TIMING = re.compile(r"myboot: timing (\S+) (\d+)us total=(\d+)us")


def parse_line(line: str):
    """Return the event for one line, or ``None`` if the line is not a MyBoot event.

    Order matters: the more specific "handoff to ... failed" is tried before the
    generic "handing off to", and the engine-error line is tried before nothing.
    """
    m = _SCAN.search(line)
    if m:
        return ev.VolumeScan(count=int(m.group(1)))

    m = _FSPROBE.search(line)
    if m:
        return ev.FsProbe(path=m.group(1), exists=(m.group(2) == "true"))

    m = _DISCOVERED.search(line)
    if m:
        return ev.Discovered(total=int(m.group(1)), bootable=int(m.group(2)))

    m = _ENTRY.search(line)
    if m:
        loader = m.group(3) if m.group(2).startswith("Some") else None
        return ev.Entry(title=m.group(1), loader=loader, health=m.group(4))

    m = _HANDOFF_FAILED.search(line)
    if m:
        return ev.HandoffFailed(entry_id=m.group(1), error=m.group(2))

    m = _HANDOFF.search(line)
    if m:
        return ev.HandOff(entry_id=m.group(1))

    m = _NO_PLAN.search(line)
    if m:
        return ev.NoLaunchPlan(entry_id=m.group(1))

    m = _NO_CAND.search(line)
    if m:
        return ev.NoCandidate(tried=int(m.group(1)))

    m = _GOP.search(line)
    if m:
        return ev.GopUnavailable(reason=m.group(1))

    m = _ENGINE_ERR.search(line)
    if m:
        return ev.EngineError(error=m.group(1))

    m = _TIMING.search(line)
    if m:
        return ev.Timing(stage=m.group(1), us=int(m.group(2)), total_us=int(m.group(3)))

    return None


def parse(text: str) -> List[object]:
    """Parse a whole serial capture into an ordered list of events."""
    out: List[object] = []
    for line in text.splitlines():
        event = parse_line(line)
        if event is not None:
            out.append(event)
    return out
