# Implementation Status — COMPLETE

**UEFI target build: CONFIRMED.** All 17 workspace crates — including the
firmware-facing `platform`, `engine`, and `bin/myboot` — compile against
`x86_64-unknown-uefi` with uefi-rs 0.33, producing `myboot.efi`. The five
0.33-specific fixes are documented in `MyBoot_BUILD_AND_TROUBLESHOOT.md`.


The entire MyBoot source tree (Chapters 1–6) is implemented as production code:
real logic, no placeholders, no TODOs. `#![forbid(unsafe_code)]` on every pure
crate; the only assembly is the kernel-handoff trampoline in `arch`.

## Verification snapshot (this build)

- **104 host tests pass** across all 14 pure crates, 0 failures:
  graph 5, storage 9, ports 3, persistence 7, config 10, policy 5, health 5,
  discovery 22, transaction 8, security 4, providers 6, ui-shell 6, ui-gfx 8,
  arch 6.
- **0 TODO / stub / placeholder markers** in the whole tree (Rust and assembly).
- **host-cli** and **xtask** compile and run natively (host target).
- Every pure crate forbids `unsafe`; `unsafe` is confined to `arch`, `platform`,
  and the direct-boot handoff in `engine`, each documented with its precondition.

## What changed in the audit/rework pass

This pass responded to a review that (correctly) found stubs and gaps. Each was
fixed with evidence, not patched over:

1. **Assembly was a stub with a `TODO`.** `arch/x86_64/handoff.s` is now a
   complete Linux 64-bit boot-protocol trampoline: it loads a 4 GiB-flat GDT
   (`__BOOT_CS`=0x10, `__BOOT_DS`=0x18), sets the segment registers, installs the
   kernel stack, puts `boot_params` in `%rsi`, reloads `CS` via a far return, and
   jumps to the entry point — per Documentation/arch/x86/boot.rst. `cpu.rs` no
   longer has a TODO. A new host-tested `arch/x86_64/boot_params.rs` models the
   zero page, `setup_header`, and E820 exactly. (Also fixed a real bug: Rust's
   `global_asm!` defaults to Intel syntax, so the AT&T file needs `.att_syntax
   prefix`.)

2. **The direct-kernel path was unreachable.** `engine/src/direct_boot.rs` now
   implements it fully: read the kernel, validate its `setup_header`, place the
   protected-mode body, load the initrd and cmdline, build `boot_params`, exit
   boot services, convert the UEFI memory map to E820, and hand off via `arch`.
   `discovery/src/refine.rs` selects `LinuxDirect` vs `LinuxEfiStub` by peeking
   the kernel's PE header, so the path is reached only when genuinely needed.

3. **Distro discovery was missing.** `discovery/src/distros.rs` scans the ESP's
   `\EFI\<vendor>\` directories and identifies Ubuntu, Kubuntu, Debian, Kali,
   Parrot, Zorin, Mint, Pop!_OS, Fedora, RHEL, CentOS, Rocky, Alma, openSUSE,
   Arch, EndeavourOS, Manjaro, Gentoo, NixOS, Void, MX and more — preferring
   `shimx64.efi` (Secure Boot), reading `os-release` `PRETTY_NAME` for the exact
   edition when present on the ESP.

4. **`config.toml` creation was undocumented.** `myboot install` (host-cli) now
   creates `EFI/MyBoot/config.toml` on the ESP, copies the loader to
   `EFI/MyBoot/BOOTX64.EFI` and the removable fallback, and prints the exact
   `efibootmgr` command. It never overwrites an existing config.

5. **God-file `lib.rs` modules split by responsibility (SRP):**
   - `graph` → `ids`, `kinds`, `health`, `entry`, `tree` (+ re-exporting `lib`).
   - `ports` → `error`, `traits`, `mock` (test doubles separated from contracts).
   - `policy` → `model` (data) + `decide` (algorithm).
   - `health` is deliberately kept as one file: a single cohesive responsibility
     (assess boot health); splitting it would create microscopic files. The test
     is cohesion, not line count.
   Every public path (`graph::X`, `ports::X`, `policy::X`) is unchanged via
   re-exports, proven by the full regression above.

## References consulted (and where each landed in code)

- **kernel.org `Documentation/arch/x86/boot.rst`** (64-bit boot protocol) →
  `arch/x86_64/handoff.s`, `arch/x86_64/boot_params.rs`.
- **Linux `arch/x86/kernel/head_64.S`** (canonical flat GDT descriptors) →
  `handoff.s`.
- **LWN + kernel EFI list** (EFI handover protocol deprecated; EFI-stub is the
  modern path) → `engine/pipeline.rs`, `engine/direct_boot.rs` design notes.
- **Gentoo wiki "EFI System Partition" + efibootmgr** (`\EFI\<vendor>\` layout) →
  `discovery/distros.rs`.
- **systemd-boot + rEFInd** (enumerate installed systems, shim-first ordering) →
  `discovery/esp.rs`, `discovery/distros.rs`.
- **Rust OSDev `bootloader`, GRUB, systemd, NixOS manual** — studied for the
  overall model (boot protocols, chainloading, UKI/BLS entries, generations);
  reflected in `providers/`, `discovery/bls.rs`, `discovery/bootspec.rs`.

## Not testable in this sandbox (compiled/tested by you)

`platform`, `ui-gfx`'s firmware glue, `engine`, and `bin/myboot` depend on
uefi-rs and the `x86_64-unknown-uefi` target, which the sandbox toolchain lacks.
They are written against uefi-rs 0.33. Build and test them per
`MyBoot_BUILD_AND_TROUBLESHOOT.md`.

---

## Runtime hardening (post first-boot on firmware)

Once `myboot.efi` ran on OVMF, live testing surfaced runtime behaviour the host
tests could not. Two fixes, both verified:

1. **Un-launchable entries dropped (`discovery/build`).** A firmware `Boot####`
   variable can name a whole device, producing an entry with no loader and no
   kernel. `build_graph` now drops entries with neither, after merging, so a real
   entry that gains a loader from another source is kept. (Discovery: 23 tests.)

2. **Never dead-end handoff (`engine/pipeline`).** The engine no longer commits
   to a single entry. It builds an ordered candidate list — the chosen entry
   first, then the remaining bootable entries healthiest-first — and tries each
   through the transaction + handoff until one hands off (`launch` only returns
   on failure). Only when every candidate is exhausted does it return an error
   for the caller to drop to firmware/recovery. `plan_for` is checked before an
   attempt is charged, so an unplannable entry never wastes a try.

Total host tests: **105**, 0 failures.

## Boot-time scan performance (Challenge 1, Phase A — 2026-10-24)

Full details and numbers are in `TESTLOG.md`.

- **Stage timing:** `--features engine/boot-timing` logs
  `myboot: timing <stage> <us>us total=<us>us`. `./manage.py bench` runs it in QEMU,
  optionally with a NixOS-shaped fixture (`--generations N --kernels K`).
- **Discovery no longer reads whole files to inspect headers.**
  - `refine` reads only the PE header, once per distinct kernel.
  - `bootspec` skips files.
  - The whole-loader diagnostic read is gone.
- **Health checks each entry against its own volume**, which fixes a cross-disk false
  "Healthy". `Volume` ids are computed once (byte-identical). `write_in` truncates.
- **QEMU results, 100 BLS generations:** discovery 135 s → 263 ms, entry set identical.
  Host tests: **127**, 0 failures. Smoke: 6/6.
- **Not done:**
  - Phase B (the discovery cache) is waiting on real-hardware timing.
  - A7 (narrowing controller connect) and A8 (BootOrder-only variable scan) were skipped
    by decision.

## Real-OS chainload testing

**ISO test PROVEN on QEMU/OVMF (2026-10-24).** Under MyBoot, the Ubuntu 26.04.1 server ISO
booted to its installer and the Parrot 7.3 ISO booted to its live desktop. In both cases
the ISO's loader was chainloaded by device path from the ISO's own volume and found its
own GRUB config, kernel and initrd. Real hardware, installed OSes and Secure Boot are
still unproven. Details are in `TESTLOG.md`.

the `./manage.py smoke` command picks a real, different EFI application to chainload as the
"Ubuntu" entry (proving a genuine handoff, not the self-recursion): it uses
`$MYBOOT_REAL_LOADER`, else the UEFI shell (`edk2-uefi-shell`, now in the flake as
`$MYBOOT_UEFI_SHELL`), else the host's `systemd-bootx64.efi`, else a myboot.efi
self-copy. Selecting that entry launches the real app on firmware.

---

## Tooling: Python CLI (`pyboot/`, replaces the shell scripts)

The shell scripts have been replaced by a single, decentralized Python CLI
(`./manage.py`, package `pyboot/`, one responsibility per module). It builds,
drives QEMU via `pexpect`, **parses MyBoot's serial console and asserts pass/fail**
(the pattern QEMU's and U-Boot's own suites use), and installs to real media with
hard safety guards (removable-only USB; UEFI/NixOS install gates; delegates the
coexisting ESP copy to the tested Rust host CLI). Pure logic (`events`, `logparse`,
`assertions`) is unit-tested without QEMU (`python -m pytest tools/tests`).
Commands: `test build smoke iso-test check verify install uninstall usb`.
