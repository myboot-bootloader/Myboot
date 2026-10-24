"""Stage the synthetic EFI System Partition used by the QEMU tests.

ONE job: lay out a directory that QEMU serves as a FAT volume, containing MyBoot
(as the fallback loader and under ``\\EFI\\MyBoot``) plus a few discoverable "OS"
loaders. One entry uses a real, different EFI application when available, so a
handoff can be exercised end to end.
"""
from __future__ import annotations

import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from . import console, paths

_CONFIG = """\
default = "windows:default"
timeout_secs = 8
policy = "last-good-then-default"
confirm = true
max_tries = 3
rollback = "last-good"
"""

# ISO test: a dynamic default that matches no decoy (there are none), so policy
# picks the healthiest real entry; longer timeout so you can read the menu and
# pick the ISO.
_CONFIG_MINIMAL = """\
default = "auto"
timeout_secs = 20
policy = "last-good-then-default"
confirm = true
max_tries = 3
rollback = "last-good"
"""


@dataclass(frozen=True)
class StagedEsp:
    root: Path
    real_loader_used: Optional[Path]


def stage(efi_binary: Path, real_loader: Optional[Path], *, minimal: bool = False,
          nixos_generations: int = 0, nixos_kernels: int = 1) -> StagedEsp:
    """Create the synthetic ESP under ``build/esp`` and return where it is.

    Default: stage a few discoverable "OS" decoys (backed by a real EFI app) for the
    self-contained SMOKE test. ``minimal=True``: stage ONLY MyBoot (fallback + its
    own dir) — used by the ISO test so the real OS on the attached ISO is the only
    real target and the menu isn't polluted with decoys. ``nixos_generations > 0``
    adds the NixOS-shaped benchmark fixture (see :func:`_stage_nixos_fixture`).
    """
    esp = paths.esp_dir()
    if esp.exists():
        shutil.rmtree(esp)

    if minimal:
        (esp / "EFI/BOOT").mkdir(parents=True, exist_ok=True)
        (esp / "EFI/MyBoot").mkdir(parents=True, exist_ok=True)
        # MyBoot at the removable fallback path so firmware boots it, plus its own
        # dir. No decoys: the only real OS is whatever is on the attached media.
        shutil.copy2(efi_binary, esp / "EFI/BOOT/BOOTX64.EFI")
        shutil.copy2(efi_binary, esp / "EFI/MyBoot/BOOTX64.EFI")
        (esp / "EFI/MyBoot/config.toml").write_text(_CONFIG_MINIMAL, encoding="utf-8")
        console.success(f"staged minimal ESP (MyBoot only) at {esp.relative_to(paths.repo_root())}")
        return StagedEsp(root=esp, real_loader_used=None)

    for sub in ("EFI/BOOT", "EFI/MyBoot", "EFI/Microsoft/Boot", "EFI/ubuntu"):
        (esp / sub).mkdir(parents=True, exist_ok=True)

    # The default-picked entry (Windows, healthiest-first) gets the REAL loader so
    # the first handoff launches a genuine, different EFI app (the shell) and stops
    # — rather than a myboot.efi self-copy, which would chainload MyBoot into itself
    # forever and never reach a real target. Ubuntu also gets it, so the test is
    # robust to graph-order changes; the \EFI\BOOT fallback stays MyBoot (it is the
    # boot medium).
    shutil.copy2(efi_binary, esp / "EFI/BOOT/BOOTX64.EFI")
    shutil.copy2(efi_binary, esp / "EFI/MyBoot/BOOTX64.EFI")
    shutil.copy2(efi_binary, esp / "EFI/ubuntu/grubx64.efi")

    if real_loader is not None:
        shutil.copy2(real_loader, esp / "EFI/Microsoft/Boot/bootmgfw.efi")
        shutil.copy2(real_loader, esp / "EFI/ubuntu/shimx64.efi")
        console.info(f"real chainload target for the default entry: {real_loader}")
    else:
        shutil.copy2(efi_binary, esp / "EFI/Microsoft/Boot/bootmgfw.efi")
        shutil.copy2(efi_binary, esp / "EFI/ubuntu/shimx64.efi")
        console.info("no real loader found; entries are myboot.efi self-copies")

    (esp / "EFI/ubuntu/os-release").write_text(
        'NAME=Ubuntu\nPRETTY_NAME="Ubuntu 24.04 LTS"\n', encoding="utf-8"
    )
    (esp / "EFI/MyBoot/config.toml").write_text(_CONFIG, encoding="utf-8")

    if nixos_generations > 0:
        _stage_nixos_fixture(esp, real_loader or efi_binary, nixos_generations, nixos_kernels)

    console.success(f"staged ESP at {esp.relative_to(paths.repo_root())}")
    return StagedEsp(root=esp, real_loader_used=real_loader)


# Size of each fixture kernel: a typical distro bzImage is 10-15 MB. Reading the
# whole file vs. just its PE header is exactly the cost being measured.
_KERNEL_BYTES = 12 * 1024 * 1024
_INITRD_BYTES = 64 * 1024


def _stage_nixos_fixture(esp: Path, pe_image: Path, generations: int, kernels: int) -> None:
    """Lay out what systemd-boot leaves on a NixOS ESP: one BLS entry per
    generation in ``\\loader\\entries``, all pointing at a few shared kernels and
    initrds in ``\\EFI\\nixos``. Kernels are a real PE image padded to a realistic
    size so discovery classifies them as EFI stubs. Benchmark use only: the smoke
    test's entry-count assertions do not apply to an ESP staged with this.
    """
    kernels = max(1, min(kernels, generations))
    nixos = esp / "EFI/nixos"
    entries = esp / "loader/entries"
    nixos.mkdir(parents=True, exist_ok=True)
    entries.mkdir(parents=True, exist_ok=True)
    image = pe_image.read_bytes()
    pairs = []
    for k in range(kernels):
        tag = f"{k:032x}"
        kname = f"{tag}-linux-6.12.{k}-bzImage.efi"
        iname = f"{tag}-initrd-linux-6.12.{k}-initrd.efi"
        (nixos / kname).write_bytes(image + bytes(max(0, _KERNEL_BYTES - len(image))))
        (nixos / iname).write_bytes(bytes(_INITRD_BYTES))
        pairs.append((kname, iname))
    for g in range(1, generations + 1):
        kname, iname = pairs[g % kernels]
        (entries / f"nixos-generation-{g}.conf").write_text(
            "title NixOS\n"
            f"version Generation {g} NixOS 26.05 (Linux 6.12.{g % kernels}), built on 2026-10-01\n"
            f"linux /efi/nixos/{kname}\n"
            f"initrd /efi/nixos/{iname}\n"
            f"options init=/nix/store/{g:032x}-nixos-system/init loglevel=4\n"
            "machine-id 00000000000000000000000000000000\n",
            encoding="utf-8",
        )
    console.info(f"NixOS fixture: {generations} generations sharing {kernels} kernel(s)")
