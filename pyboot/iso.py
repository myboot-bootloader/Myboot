"""The real-OS test: attach one or MORE real distro ISOs and boot them from MyBoot.

Stages a MINIMAL ESP (MyBoot only — no decoys), then attaches every ISO you pass
as its OWN volume. MyBoot scans them all, excludes its own loader, and lists each
ISO as a separate entry — the true n-OS test across many volumes at once. Selecting
one chainloads that ISO's loader FROM ITS OWN VOLUME via device path, so it finds
its own kernel and boots.

Modes:
  --ui    (default) open a QEMU window and pick an OS from MyBoot's menu, watch it
          boot, close the window, and repeat for the next — the honest n-OS test.
  --no-ui headless: assert MyBoot scanned all volumes and discovered one entry per
          ISO.
"""
from __future__ import annotations

from pathlib import Path
from typing import List

from . import build, esp, qemu, logparse, assertions as A, report, console
from .env import Env
from .errors import EnvironmentError_, AssertionFailure


def run(env: Env, iso_paths: List[str], *, ui: bool = True, timeout: int = 120) -> None:
    isos = [Path(p) for p in iso_paths]
    for iso in isos:
        if not iso.is_file():
            raise EnvironmentError_(f"ISO not found: {iso}")

    efi = build.build_efi()
    staged = esp.stage(efi, None, minimal=True)   # no decoys — the ISOs are the OSes

    names = ", ".join(i.name for i in isos)
    if ui:
        console.section(f"ISO boot test — interactive ({len(isos)} ISO(s))")
        console.info(f"attached read-only: {names}")
        console.info("")
        console.info("A QEMU WINDOW will open with MyBoot's menu. MyBoot excludes its OWN")
        console.info(f"loader, so the menu lists one entry per ISO ({len(isos)} total).")
        console.info("Select one and watch that distro boot; close the window and run")
        console.info("again to try the next. Each entry lives on its own volume — this")
        console.info("is the n-OS test proving MyBoot lists and boots many OSes at once.")
        console.info("")
        qemu.launch_interactive(env, staged.root, isos=isos)
        console.success("QEMU exited. Each ISO that booted its installer proves a real, "
                        "cross-volume chainload.")
        return

    console.step(f"attaching {len(isos)} ISO(s) (headless discovery check): {names}")
    result = qemu.run(env, staged.root, timeout=timeout, isos=isos,
                      # Match the WHOLE line: pexpect stops capturing at the end of
                      # the match, and logparse needs "... N bootable" to parse it.
                      stop_on=[r"discovered \d+ entr\S* \d+ bootable", r"no candidate could be booted"])
    events = logparse.parse(result.output)
    checks = [
        A.scanned_at_least(events, 1 + len(isos)),      # boot ESP + each ISO
        A.discovered_bootable(events, len(isos)),        # one entry per ISO (self excluded)
    ]
    ok = report.render("ISO discovery", checks)
    console.info("headless mode verifies discovery; run with --ui to pick an OS and boot it.")
    if not ok:
        raise AssertionFailure("ISO discovery check failed")
