"""The parsed-event data model.

MyBoot prints structured diagnostics over the serial console. This module defines
the typed events those lines map to. It has ONE job: describe the data. It contains
no parsing (see ``logparse``) and no policy (see ``assertions``), so it stays a
dependency-free, trivially testable core.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass(frozen=True)
class VolumeScan:
    """`myboot: scanning N volume(s)`"""
    count: int


@dataclass(frozen=True)
class FsProbe:
    """`myboot: fs probe <path> exists=<bool>`"""
    path: str
    exists: bool


@dataclass(frozen=True)
class Discovered:
    """`myboot: discovered N entr(y/ies), M bootable`"""
    total: int
    bootable: int


@dataclass(frozen=True)
class Entry:
    """`myboot:   '<title>' loader=<Some("..")|None> health=<Health>`"""
    title: str
    loader: Optional[str]
    health: str


@dataclass(frozen=True)
class HandOff:
    """`myboot: handing off to '<id>'`"""
    entry_id: str


@dataclass(frozen=True)
class HandoffFailed:
    """`myboot: handoff to '<id>' failed (<error>); trying next`"""
    entry_id: str
    error: str


@dataclass(frozen=True)
class NoLaunchPlan:
    """`myboot: no launch plan for '<id>'; skipping`"""
    entry_id: str


@dataclass(frozen=True)
class NoCandidate:
    """`myboot: no candidate could be booted (N tried)`"""
    tried: int


@dataclass(frozen=True)
class GopUnavailable:
    """`myboot: GOP unavailable (<reason>); selecting headlessly`"""
    reason: str


@dataclass(frozen=True)
class EngineError:
    """`myboot: engine could not hand off: <error>` (from bin/myboot)"""
    error: str


@dataclass(frozen=True)
class Timing:
    """`myboot: timing <stage> <us>us total=<us>us` (``boot-timing`` builds only)"""
    stage: str
    us: int
    total_us: int
