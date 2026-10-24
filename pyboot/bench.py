"""Boot-stage timing benchmark in QEMU (Challenge 1, Phase A0).

ONE job: compose build (with ``engine/boot-timing``) → stage (optionally with the
NixOS-shaped fixture) → run → parse → print per-stage timings, over several runs.
It measures RELATIVE change between builds. QEMU serves the ESP from a host
directory (vvfat), so absolute numbers do not carry over to real NVMe/SATA.
"""
from __future__ import annotations

from statistics import median
from typing import Dict, List

from . import build, esp, qemu, logparse, report, console, paths
from . import events as ev
from .env import Env, real_loader
from .errors import AssertionFailure

TIMING_FEATURE = "engine/boot-timing"


def median_by_stage(runs: List[List[ev.Timing]]) -> List[ev.Timing]:
    """Per-stage medians across runs, in first-seen stage order. Pure."""
    order: List[str] = []
    us: Dict[str, List[int]] = {}
    total: Dict[str, List[int]] = {}
    for run in runs:
        for t in run:
            if t.stage not in us:
                order.append(t.stage)
                us[t.stage], total[t.stage] = [], []
            us[t.stage].append(t.us)
            total[t.stage].append(t.total_us)
    return [ev.Timing(stage=s, us=int(median(us[s])), total_us=int(median(total[s])))
            for s in order]


def run(env: Env, *, generations: int, kernels: int, runs: int, timeout: int) -> None:
    """Boot ``runs`` times and print each run's stage table plus the medians."""
    efi = build.build_efi([TIMING_FEATURE])
    staged = esp.stage(efi, real_loader(),
                       nixos_generations=generations, nixos_kernels=kernels)

    all_runs: List[List[ev.Timing]] = []
    for i in range(1, runs + 1):
        result = qemu.run(env, staged.root, timeout=timeout,
                          stop_on=[r"myboot: timing to-menu \d+us total=\d+us"])
        log = paths.repo_root() / "build" / f"bench-run{i}.log"
        log.write_text(result.output, encoding="utf-8")
        events = logparse.parse(result.output)
        timings = [e for e in events if isinstance(e, ev.Timing)]
        found = next((e for e in events if isinstance(e, ev.Discovered)), None)
        report.render_timings(f"run {i}/{runs}", timings)
        if found is not None:
            console.info(f"discovered {found.total} entr(y/ies), {found.bootable} bootable")
        console.info(f"serial log: {log.relative_to(paths.repo_root())}")
        if not any(t.stage == "to-menu" for t in timings):
            raise AssertionFailure(f"run {i}: never reached the 'to-menu' timing line")
        all_runs.append(timings)

    report.render_timings(f"median of {runs} run(s)", median_by_stage(all_runs))
