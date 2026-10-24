"""The command-line interface: argument parsing and dispatch only.

ONE job: map subcommands to the module that does the work. No business logic lives
here — each handler is a few lines that resolve the environment and delegate. Exit
codes: 0 success, 1 a real test/verification failure, 2 an environment/safety
problem, 130 interrupted.
"""
from __future__ import annotations

import argparse
import sys

from . import console
from .errors import (
    PybootError, AssertionFailure, SafetyError, EnvironmentError_, BuildError, QemuError,
)


def _env():
    from .env import resolve
    return resolve()


def _cmd_test(args) -> int:
    from . import build, env
    build.host_tests(env.host_triple())
    return 0


def _cmd_build(args) -> int:
    from . import build
    build.build_efi()
    return 0


def _cmd_smoke(args) -> int:
    from . import smoke
    smoke.run(_env(), timeout=args.timeout)
    return 0


def _cmd_bench(args) -> int:
    from . import bench
    bench.run(_env(), generations=args.generations, kernels=args.kernels,
              runs=args.runs, timeout=args.timeout)
    return 0


def _cmd_iso(args) -> int:
    from . import iso
    iso.run(_env(), args.iso, ui=args.ui, timeout=args.timeout)
    return 0


def _cmd_install(args) -> int:
    from . import install
    install.install(_env(), esp=args.esp, register=args.register,
                    source=args.source, repo=args.repo, tag=args.tag)
    return 0


def _cmd_uninstall(args) -> int:
    from . import install
    install.uninstall(_env(), esp=args.esp)
    return 0


def _cmd_usb(args) -> int:
    from . import usb
    usb.make_usb(args.device, assume_yes=args.yes)
    return 0


def _cmd_verify(args) -> int:
    from . import verify
    return 0 if verify.run() else 1


def _cmd_check(args) -> int:
    """Everything that is safe and automated: host tests, build, smoke test."""
    from . import build, env, smoke
    build.host_tests(env.host_triple())
    smoke.run(_env(), timeout=args.timeout)
    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="pyboot",
        description="MyBoot build/test/install tooling (replaces the shell scripts).",
    )
    sub = p.add_subparsers(dest="command", required=True)

    sp = sub.add_parser("test", help="run the pure-crate host tests")
    sp.set_defaults(func=_cmd_test)

    sp = sub.add_parser("build", help="build myboot.efi for the UEFI target")
    sp.set_defaults(func=_cmd_build)

    sp = sub.add_parser("smoke", help="build + boot the synthetic ESP in QEMU and assert")
    sp.add_argument("--timeout", type=int, default=40, help="QEMU time box (seconds)")
    sp.set_defaults(func=_cmd_smoke)

    sp = sub.add_parser("bench", help="time each boot stage in QEMU (boot-timing build)")
    sp.add_argument("--generations", type=int, default=100,
                    help="NixOS-shaped fixture: BLS generations to stage (0 = smoke ESP only)")
    sp.add_argument("--kernels", type=int, default=4, help="distinct kernels the generations share")
    sp.add_argument("--runs", type=int, default=3, help="boots to time (medians are reported)")
    sp.add_argument("--timeout", type=int, default=120, help="per-run QEMU time box (seconds)")
    sp.set_defaults(func=_cmd_bench)

    sp = sub.add_parser("iso-test", help="boot a real distro ISO in QEMU (the real proof)")
    sp.add_argument("iso", nargs="+", help="one or more bootable distro ISOs (each becomes its own volume)")
    sp.add_argument("--ui", action=argparse.BooleanOptionalAction, default=True,
                    help="open a QEMU window to drive yourself (default); --no-ui runs headless")
    sp.add_argument("--timeout", type=int, default=90, help="headless time box (seconds)")
    sp.set_defaults(func=_cmd_iso)

    sp = sub.add_parser("check", help="host tests + build + smoke test (safe, automated)")
    sp.add_argument("--timeout", type=int, default=40)
    sp.set_defaults(func=_cmd_check)

    sp = sub.add_parser("verify", help="run Kani proofs and the TLA+ model check")
    sp.set_defaults(func=_cmd_verify)

    sp = sub.add_parser("install", help="install MyBoot to this machine's ESP (coexists)")
    sp.add_argument("--esp", default=None, help="ESP mount point (auto-detected if omitted)")
    sp.add_argument("--register", action="store_true", help="also add a firmware boot entry")
    sp.add_argument("--from", dest="source", choices=["github", "build"], default="github",
                    help="get myboot.efi from a GitHub release (default) or build locally")
    sp.add_argument("--repo", default=None, help="GitHub repo owner/name (or set MYBOOT_REPO)")
    sp.add_argument("--tag", default=None, help="release tag to install (default: latest)")
    sp.set_defaults(func=_cmd_install)

    sp = sub.add_parser("uninstall", help="remove MyBoot from this machine's ESP")
    sp.add_argument("--esp", default=None)
    sp.set_defaults(func=_cmd_uninstall)

    sp = sub.add_parser("usb", help="write MyBoot to a REMOVABLE USB stick (guarded)")
    sp.add_argument("--device", required=True, help="removable block device, e.g. /dev/sdb")
    sp.add_argument("--yes", action="store_true", help="skip the typed confirmation")
    sp.set_defaults(func=_cmd_usb)

    return p


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return args.func(args)
    except (SafetyError, EnvironmentError_) as exc:
        console.error(str(exc))
        return 2
    except (AssertionFailure, BuildError, QemuError) as exc:
        console.error(str(exc))
        return 1
    except PybootError as exc:
        console.error(str(exc))
        return 1
    except KeyboardInterrupt:
        console.warn("interrupted")
        return 130


if __name__ == "__main__":
    sys.exit(main())
