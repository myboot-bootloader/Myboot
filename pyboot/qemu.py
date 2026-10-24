"""Launch QEMU and capture MyBoot's serial console.

ONE job: run one bounded QEMU session and return everything MyBoot printed. Uses
pexpect so the run is time-boxed and terminated cleanly (MyBoot's synthetic test
loops, so it never exits on its own). Optional media (an ISO or a disk) can be
attached for the real-OS test. This module captures; it does not judge (that is
``assertions``).
"""
from __future__ import annotations

import shutil
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional, Tuple

from . import console
from .env import Env
from .errors import QemuError


@dataclass(frozen=True)
class QemuResult:
    output: str
    timed_out: bool


def _build_command(env: Env, esp_root: Path, vars_copy: Path,
                   isos: List[Path], disk: Optional[Path], display: bool) -> List[str]:
    cmd = [
        "qemu-system-x86_64",
        "-enable-kvm",
        "-m", "2048",
        "-drive", f"if=pflash,format=raw,readonly=on,file={env.ovmf_code}",
        "-drive", f"if=pflash,format=raw,file={vars_copy}",
        # MyBoot's synthetic ESP gets the HIGHEST boot priority (bootindex=0) so the
        # firmware always launches MyBoot first — even when another bootable device
        # (an ISO or disk) is attached. Without this, OVMF may boot the ISO directly
        # and MyBoot never runs.
        "-drive", f"if=none,id=espdrive,format=raw,file=fat:rw:{esp_root}",
        "-device", "ide-hd,drive=espdrive,bootindex=0",
        "-serial", "stdio",
        "-net", "none",
    ]
    if not display:
        cmd += ["-display", "none"]
    # Each ISO becomes its OWN volume so MyBoot discovers and lists them all (the
    # n-OS test). A SCSI HBA scales past IDE's 4-device limit, so 1..N ISOs work.
    if isos:
        cmd += ["-device", "virtio-scsi-pci,id=scsi0"]
        for i, iso in enumerate(isos):
            cmd += ["-drive", f"if=none,id=iso{i},format=raw,readonly=on,file={iso}",
                    "-device", f"scsi-cd,drive=iso{i},bus=scsi0.0,bootindex={i + 1}"]
    if disk is not None:
        # Attached read-only (safe for a physical device like a Ventoy USB).
        cmd += ["-drive", f"if=none,id=diskdrive,format=raw,readonly=on,file={disk}",
                "-device", f"virtio-blk-pci,drive=diskdrive,bootindex={len(isos) + 1}"]
    return cmd


def launch_interactive(
    env: Env,
    esp_root: Path,
    *,
    isos: Optional[List[Path]] = None,
    disk: Optional[Path] = None,
) -> int:
    """Launch QEMU WITH a graphical display for a human to drive.

    Unlike :func:`run`, this does NOT capture or time-box: MyBoot renders its real
    menu in the window, you select an entry and watch it boot, and QEMU runs until
    you quit it. Serial still streams to this terminal so you can read MyBoot's
    discovery log. Returns QEMU's exit code.
    """
    import subprocess
    tmp = Path(tempfile.mkdtemp(prefix="myboot-qemu-"))
    vars_copy = tmp / "OVMF_VARS.fd"
    shutil.copy2(env.ovmf_vars, vars_copy)
    cmd = _build_command(env, esp_root, vars_copy, isos or [], disk, display=True)
    console.step("launching QEMU with a display — drive it yourself (close the window to quit)")
    console.info("$ " + " ".join(cmd))
    try:
        return subprocess.run(cmd).returncode
    finally:
        try:
            shutil.rmtree(tmp)
        except OSError:
            pass


def run(
    env: Env,
    esp_root: Path,
    *,
    timeout: int = 40,
    isos: Optional[List[Path]] = None,
    disk: Optional[Path] = None,
    display: bool = False,
    stop_on: Optional[List[str]] = None,
) -> QemuResult:
    """Run QEMU until a ``stop_on`` marker is seen or ``timeout`` elapses.

    Returns the full serial capture. Requires ``pexpect``; a clear error explains
    how to get it (it is provided by the dev shell) if it is missing.
    """
    try:
        import pexpect  # imported lazily so pure-logic tests need no dependency
    except ImportError as exc:  # pragma: no cover - environment guard
        raise QemuError(
            "the 'pexpect' module is required to drive QEMU. It is provided by the "
            "dev shell (python3Packages.pexpect); enter it with `direnv allow`."
        ) from exc

    # Firmware needs writable NVRAM: copy OVMF_VARS to a throwaway file.
    tmp = Path(tempfile.mkdtemp(prefix="myboot-qemu-"))
    vars_copy = tmp / "OVMF_VARS.fd"
    shutil.copy2(env.ovmf_vars, vars_copy)

    cmd = _build_command(env, esp_root, vars_copy, isos or [], disk, display)
    console.step("launching QEMU (bounded, serial captured)")
    console.info("$ " + " ".join(cmd))

    child = pexpect.spawn(cmd[0], cmd[1:], encoding="utf-8", timeout=timeout)
    try:
        output, timed_out = _capture(child, stop_on or [])
    finally:
        if child.isalive():
            child.terminate(force=True)
        try:
            shutil.rmtree(tmp)
        except OSError:
            pass

    return QemuResult(output=output, timed_out=timed_out)


# How long to wait for the rest of a line after a stop marker matched mid-line.
LINE_GRACE_SECS = 2


def _capture(child, markers: List[str]) -> Tuple[str, bool]:
    """Read ``child`` until EOF, its timeout, or a ``markers`` match.

    Returns ``(output, timed_out)``. After a marker matches, the rest of THAT
    line is read too (bounded by ``LINE_GRACE_SECS``): pexpect stops at the end
    of the match, and ``logparse`` needs whole lines (e.g. ``... 2 bootable``,
    ``... (3 tried)``). A marker with no line ending still returns promptly.
    """
    import pexpect  # lazy, as in run(): pure-logic tests need no dependency

    captured: List[str] = []
    idx = child.expect([pexpect.EOF, pexpect.TIMEOUT] + markers)
    captured.append(child.before or "")
    if idx == 0:  # EOF
        return "".join(captured), False
    if idx == 1:  # TIMEOUT
        return "".join(captured), True
    # A marker matched: keep it, then finish its line.
    captured.append(child.after or "")
    rest = child.expect([r"\r?\n", pexpect.EOF, pexpect.TIMEOUT], timeout=LINE_GRACE_SECS)
    captured.append(child.before or "")
    if rest == 0:
        captured.append(child.after or "")
    return "".join(captured), False
