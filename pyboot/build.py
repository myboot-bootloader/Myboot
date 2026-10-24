"""Build and host-test the Rust workspace.

ONE job: invoke cargo. Two things: run the pure-crate host tests, and build the
firmware binary. It does not stage an ESP or launch QEMU.
"""
from __future__ import annotations

from pathlib import Path
from typing import List, Optional

from . import console, proc, paths
from .errors import BuildError

# The firmware-free crates that test natively on the host toolchain.
PURE_CRATES: List[str] = [
    "graph", "storage", "ports", "persistence", "config", "policy", "health",
    "discovery", "transaction", "security", "providers", "ui-shell", "ui-gfx", "arch",
]


def host_tests(host_triple: str) -> None:
    """Run ``cargo test`` for each pure crate against the host target.

    Raises :class:`BuildError` on the first failing crate.
    """
    root = str(paths.repo_root())
    failures: List[str] = []
    for crate in PURE_CRATES:
        console.section(f"testing {crate}")
        res = proc.run(
            ["cargo", "test", "-p", crate, "--target", host_triple],
            cwd=root, check=False,
        )
        if not res.ok:
            failures.append(crate)
    if failures:
        raise BuildError(f"host tests failed for: {', '.join(failures)}")
    console.success(f"all {len(PURE_CRATES)} pure-crate test suites passed")


def build_efi(features: Optional[List[str]] = None) -> Path:
    """Build ``myboot.efi`` for the UEFI target and return its path.

    ``features`` are passed to cargo verbatim (e.g. ``["engine/boot-timing"]``).
    """
    console.step("building myboot.efi (release, x86_64-unknown-uefi)")
    cmd = ["cargo", "build", "-p", "myboot", "--release", "--target", paths.EFI_TARGET]
    if features:
        cmd += ["--features", ",".join(features)]
    proc.run(cmd, cwd=str(paths.repo_root()))
    efi = paths.efi_binary()
    if not efi.is_file():
        raise BuildError(f"build reported success but {efi} is missing")
    _verify_pe(efi)
    console.success(f"built {efi.relative_to(paths.repo_root())}")
    return efi


def build_host_cli(host_triple: str) -> Path:
    """Build the host-side installer CLI and return the binary path."""
    console.step("building host CLI")
    proc.run(
        ["cargo", "build", "--manifest-path", str(paths.host_cli_manifest()),
         "--release", "--target", host_triple],
        cwd=str(paths.repo_root()),
    )
    binary = paths.repo_root() / "host-cli" / "target" / host_triple / "release" / "myboot"
    if not binary.is_file():
        raise BuildError(f"host CLI build reported success but {binary} is missing")
    return binary


def _verify_pe(path: Path) -> None:
    """Confirm the artifact is a PE32+ executable (the EFI image format)."""
    with path.open("rb") as fh:
        if fh.read(2) != b"MZ":
            raise BuildError(f"{path} is not a PE image (missing MZ header)")
