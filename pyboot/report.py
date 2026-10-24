"""Render a pass/fail report from a list of checks.

ONE job: turn assertion results into human output and a process exit code. Kept
separate so the same checks can be printed here or serialized elsewhere later.
"""
from __future__ import annotations

from typing import List

from . import console
from .assertions import Check
from .events import Timing


def render(title: str, checks: List[Check]) -> bool:
    """Print each check and a summary. Return True iff all passed."""
    console.section(title)
    for c in checks:
        (console.success if c.passed else console.error)(f"{c.name}: {c.detail}")
    passed = sum(1 for c in checks if c.passed)
    total = len(checks)
    if passed == total:
        console.success(f"{title}: {passed}/{total} checks passed")
        return True
    console.error(f"{title}: {passed}/{total} checks passed")
    return False


def render_timings(title: str, timings: List[Timing]) -> None:
    """Print one row per boot stage: its own duration and the running total."""
    console.section(title)
    if not timings:
        console.warn("no timing lines captured (was the build made with boot-timing?)")
        return
    width = max(len(t.stage) for t in timings)
    for t in timings:
        console.info(f"{t.stage:<{width}}  {t.us / 1000:>10.1f} ms   total {t.total_us / 1000:>10.1f} ms")
