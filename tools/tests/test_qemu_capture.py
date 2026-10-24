"""qemu._capture over a plain shell process (no QEMU): stop markers must never
truncate the line they match."""
import os, sys, time
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))

import pytest

pexpect = pytest.importorskip("pexpect")

from pyboot import qemu, logparse
from pyboot import events as ev


def _spawn(script: str, timeout: int = 10):
    return pexpect.spawn("sh", ["-c", script], encoding="utf-8", timeout=timeout)


def test_marker_mid_line_captures_the_whole_line():
    child = _spawn("printf 'boot\\nmyboot: discovered 2 entr(y/ies), 2 bootable\\n'; sleep 5")
    out, timed_out = qemu._capture(child, [r"discovered \d+ entr"])
    child.terminate(force=True)
    assert not timed_out
    assert ev.Discovered(total=2, bootable=2) in logparse.parse(out)


def test_dead_end_line_keeps_its_count():
    child = _spawn("printf 'myboot: no candidate could be booted (3 tried)\\n'; sleep 5")
    out, _ = qemu._capture(child, [r"no candidate could be booted"])
    child.terminate(force=True)
    assert ev.NoCandidate(tried=3) in logparse.parse(out)


def test_marker_without_line_ending_returns_within_the_grace_period():
    child = _spawn("printf 'UEFI Interactive Shell v2.2'; sleep 30")
    start = time.monotonic()
    out, timed_out = qemu._capture(child, [r"UEFI Interactive Shell"])
    elapsed = time.monotonic() - start
    child.terminate(force=True)
    assert not timed_out
    assert "UEFI Interactive Shell v2.2" in out
    assert elapsed < qemu.LINE_GRACE_SECS + 2


def test_timeout_without_marker_is_reported():
    child = _spawn("printf 'nothing here\\n'; sleep 30", timeout=1)
    out, timed_out = qemu._capture(child, [r"never appears"])
    child.terminate(force=True)
    assert timed_out and "nothing here" in out
