"""More pure-logic tests: event immutability and CLI parser wiring (no QEMU)."""
import os, sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))

import pytest  # noqa: F401  (also runnable via the inline runner below)
from pyboot import events as ev
from pyboot import cli


def test_events_are_frozen():
    e = ev.Discovered(total=3, bootable=3)
    try:
        e.total = 9  # type: ignore[misc]
        assert False, "events must be immutable"
    except Exception:
        pass


def test_cli_parser_has_all_commands():
    p = cli.build_parser()
    # every subcommand must parse and carry a handler
    for cmd, extra in [
        (["test"], {}), (["build"], {}), (["smoke"], {}),
        (["iso-test", "/tmp/x.iso"], {}), (["bench"], {}), (["check"], {}), (["verify"], {}),
        (["install"], {}), (["uninstall"], {}), (["usb", "--device", "/dev/sdz"], {}),
    ]:
        ns = p.parse_args(cmd)
        assert hasattr(ns, "func"), f"{cmd} has no handler"


def test_cli_requires_a_command():
    p = cli.build_parser()
    try:
        p.parse_args([])
        assert False, "a command should be required"
    except SystemExit:
        pass


def test_bench_median_by_stage_keeps_order():
    from pyboot.bench import median_by_stage
    runs = [
        [ev.Timing("connect", 10, 10), ev.Timing("discovery", 100, 110)],
        [ev.Timing("connect", 30, 30), ev.Timing("discovery", 300, 330)],
        [ev.Timing("connect", 20, 20), ev.Timing("discovery", 200, 220)],
    ]
    assert median_by_stage(runs) == [ev.Timing("connect", 20, 20), ev.Timing("discovery", 200, 220)]
