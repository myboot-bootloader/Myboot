# CLAUDE.md — Operating instructions for Claude Code on the MyBoot project

You are working on **MyBoot**, a UEFI boot manager written in Rust (`no_std`) plus
a little x86-64 assembly. Your job in this repo is to **build it, test it, and prove
it works — safely — and to keep the codebase production-complete**. These rules are
binding. Follow them to the letter. When a rule here conflicts with a request, stop
and ask.

---

## 0. Prime directives (never violate)

1. **No stubs, ever.** No `todo!()`, `unimplemented!()`, `TODO`, `FIXME`,
   placeholder returns, commented-out "to be implemented" logic, or half-wired
   features. Every function you write or touch is complete and wired end to end.
   If you cannot complete something, DO NOT fake it — stop and report.
2. **Evidence-based changes only.** For every fix, cite the concrete evidence: the
   exact compiler error, and the authoritative source (the `uefi` crate's docs on
   docs.rs for **version 0.33.0 specifically**, its CHANGELOG, or the UEFI spec).
   Never guess an API. Never "try something" speculatively. If you are not sure,
   look it up or ask. State the reason for each change in the commit/message.
3. **Change only what the evidence requires.** Do not refactor, rename, reformat,
   or "improve" unrelated code while fixing an error. Minimal, surgical diffs.
   Do not introduce a new bug or remove functionality that may be needed later.
4. **NEVER install to a real machine's internal disk.** Not the user's, not any.
   You may build, run in QEMU, and (only when explicitly told) write to a
   REMOVABLE USB device that the user has named. Installing to or modifying an
   internal disk / ESP / NVRAM boot entries is FORBIDDEN unless the user gives an
   explicit, unambiguous, per-action go-ahead naming the device. When in doubt,
   refuse and ask.
5. **Keep the pure crates' tests green.** After any change, the host test suite
   must still pass with zero failures. If your change reduces coverage or breaks a
   test, either fix it properly or revert.
6. **One responsibility per file.** New code goes in a file that does one job. Do
   not bloat an existing file with unrelated functions.
7. **Confine `unsafe`.** `unsafe` is allowed ONLY in `crates/platform`,
   `crates/arch`, and `crates/engine/src/direct_boot.rs`. Every other crate keeps
   `#![forbid(unsafe_code)]`. Do not weaken that.
8. **Report honestly.** Distinguish "proven" from "compiled but unproven." Never
   claim a real OS boots unless you have observed it boot. A self-contained app
   (the UEFI shell) launching does NOT prove a real OS loader will work.

---

## 1. What MyBoot is (context you must hold)

A UEFI boot manager: it scans every disk, builds a unified "Boot Graph" of every
bootable OS, shows a menu, and hands off to the chosen one — with a transactional
"arm a marker before boot, confirm/rollback on next boot" safety model, and a
"never dead-end" fallback that tries the next healthy entry if one fails.

Architecture is hexagonal: a pure `no_std` core (`graph`, `policy`, `health`,
`transaction`, `config`, `discovery`, `storage`, `ports`, …) depends only on the
`ports` traits (`VarStore`, `FileStore`); `crates/platform` is the single UEFI
adapter; `crates/arch` holds the assembly handoff. Resolved dependency: **uefi
0.33.0** (uefi-raw 0.9.0, uefi-macros 0.17.0). Target: `x86_64-unknown-uefi`.

Read `MyBoot_How_It_Works.md` and `IMPLEMENTATION_STATUS.md` before starting.
The current, ordered work is in **`TASKS.md`** — start there.

### Current proven / unproven status (update this as you learn)

- PROVEN: builds for the UEFI target; runs on OVMF firmware; multi-volume scan,
  discovery, health, distro detection, menu, never-dead-end fallback all execute;
  chainload successfully launches a self-contained EFI app (the UEFI shell).
- PROVEN on QEMU/OVMF (2026-10-24, see TESTLOG.md): chainloading a **real OS loader
  that must find its own files** via the device-path path. Ubuntu 26.04.1 server ISO
  reached its installer; Parrot 7.3 ISO reached its live desktop.
- UNPROVEN: the same on real hardware; installed (on-disk) OSes such as Windows Boot
  Manager or an installed shim/GRUB; Secure Boot enabled.

---

## 2. Environment

- Enter the dev shell first: `direnv allow` (or `nix develop`). This provides the
  Rust toolchain with the `x86_64-unknown-uefi` target, QEMU, OVMF, the UEFI shell
  (`$MYBOOT_UEFI_SHELL`), and disk tooling. It exports `OVMF_CODE` / `OVMF_VARS`.
- Never pass the literal example env lines from comments as real commands.

---

## 3. The canonical test procedure (run in this order, stop at first failure)

### 3.1 Host tests (pure crates) — must be 100% green
Run `./manage.py test` (pure-crate host tests). Expected: all pure-crate tests pass, 0 failures (currently ~105). If any
fail, fix properly or revert; do not proceed.

### 3.2 Build for the firmware target
```
cargo build -p myboot --release --target x86_64-unknown-uefi
file target/x86_64-unknown-uefi/release/myboot.efi   # expect: PE32+ EFI application
```
If it fails to compile, apply the **compile-fix discipline** (§5): fix exactly the
reported error against uefi-0.33 ground truth, rebuild, repeat. One error at a time.

### 3.3 QEMU smoke test (self-contained target)
```
./manage.py smoke
```
This asserts, by parsing the serial log, that ALL of these hold:
- `scanning N volume(s)` appears (N ≥ 1),
- `discovered 3 entr` with 3 bootable,
- a `handing off to '…'` line for a real entry,
- the UEFI shell banner appears (proves handoff to a real EFI app).
This proves discovery + menu + handoff. It does NOT prove real-OS boot.

### 3.4 ISO test (THE proof that matters) — real OS loader
Obtain the path to the user's real distro ISO (ask; do not guess). Run:
```
./manage.py iso-test /path/to/distro.iso
```
Drive the menu to select the ISO's entry (or let policy pick it), and assert from
the serial/console output that the **distro's own boot loader / installer actually
starts** (e.g. GRUB menu, systemd-boot, or the installer's early output) — NOT the
UEFI shell, and NOT a `LOAD_ERROR` loop. If it boots the installer, device-path
chainload is PROVEN. If it does not, capture the exact error and diagnose (§5);
the most likely area is `crates/platform/src/volume/mod.rs` (device-path build /
`LoadImageSource::FromDevicePath` / `BootPolicy`). Do not paper over it.

### 3.5 Report
Summarise: what passed, what failed, the exact evidence, and whether the
real-OS-boot claim is now PROVEN or still UNPROVEN. Update
`IMPLEMENTATION_STATUS.md`.

**Do NOT proceed to any hardware/USB/internal-disk step on your own.** Stop after
3.4 and report; hardware steps require the user's explicit go-ahead and are their
decision, done by them, not you.

---

## 4. The Python tooling (`pyboot/`) — already built; run and validate it

The shell scripts have been replaced by a Python CLI at `pyboot/` (entry point
`./manage.py`) that drives QEMU and asserts on the serial console. It is already
implemented and its pure logic is unit-tested. Your job is to RUN it on real QEMU,
fix any runtime issues you hit (per §5), and extend it only if needed — keeping the
**one responsibility per module** rule. Layout:

```
pyboot/
  manage.py            # thin entry point; wires argparse subcommands only
  pyboot/
    __init__.py
    env.py             # resolve dev-shell env: OVMF paths, uefi-shell, cargo
    build.py           # cargo build (host tests) + uefi-target build; nothing else
    esp.py             # stage the synthetic ESP directory (only that)
    qemu.py            # spawn QEMU via pexpect; send keys; capture serial (only that)
    logparse.py        # parse MyBoot serial lines into structured events (only that)
    assertions.py      # pass/fail predicates over parsed events (only that)
    iso.py             # attach/verify a real ISO for the real-OS test (only that)
    usb.py             # build a removable-USB image (guarded; never internal disk)
    report.py          # render a pass/fail report (only that)
```

Rules for the harness:
- Use `pexpect` (or QEMU QMP) to launch QEMU, send arrow/Enter keys to select a
  menu entry, and read the serial console; assert expected lines appear.
- Each subcommand (`build`, `test`, `iso-test`, `usb`, `report`) maps to a small
  function that composes the modules. No module imports unrelated concerns.
- The `test` command must return a non-zero exit code on any failed assertion, so
  it works in CI. Print a clear PASS/FAIL summary.
- `usb.py` must refuse any device that is not removable, and must never touch an
  internal disk; it requires an explicit `--device /dev/sdX` the user passed.
- Keep pure logic (logparse, assertions) importable and unit-testable WITHOUT
  QEMU, and add small pytest unit tests for them.
- Validate by actually running `./manage.py check` and `./manage.py iso-test` on
  real QEMU; fix runtime issues; keep the pure-logic unit tests green
  (`python -m pytest tools/tests`).

---

## 5. Compile-fix discipline (uefi 0.33)

The sandbox that wrote this code could not compile the UEFI target, so uefi-0.33
API mismatches are expected and normal. For each error:
1. Read the exact compiler message (it often prints the real 0.33 signature).
2. Confirm the correct API against docs.rs for `uefi` **0.33.0** or the CHANGELOG.
3. Make the minimal change to match. Add a one-line comment stating the 0.33 fact.
4. Rebuild. Fix the next error. One at a time. Never batch-guess.

Known likely spots (verify, don't assume): device-path builder usage in
`platform/src/volume/mod.rs` (`DevicePathBuilder::push`/`finalize`,
`LoadImageSource::FromDevicePath`, `BootPolicy`), `locate_handle_buffer` /
`SearchType`, trait-in-scope errors (`Identify` for `::GUID`). A cleaner
alternative to the manual node loop is `DevicePath::append_path` — use it if the
builder approach fights the 0.33 API.

---

## 6. Syncing with the architecture/review track

The design/spec/review happens in a separate assistant session against a canonical
copy of this workspace. To keep both in sync:
- Make focused commits with clear messages that state the WHY and the evidence.
- Keep a running `TESTLOG.md` at the repo root: each entry = date, what you ran,
  the exact result (paste key serial lines), and PROVEN/UNPROVEN status changes.
- When you change code to fix a build/runtime issue, note the file, the 0.33 fact,
  and the before/after in `TESTLOG.md` so it can be reviewed and folded back.
- If a change is bigger than a surgical fix (new module, behaviour change), pause
  and write a short proposal in `TESTLOG.md` first.

---

## 7. Hard "do not" list (summary)

- Do NOT install to, or modify, any internal disk / ESP / firmware NVRAM entries.
- Do NOT write stubs, placeholders, or TODOs.
- Do NOT guess a uefi API or make speculative edits.
- Do NOT claim a real OS boots unless you observed it boot (shell ≠ OS).
- Do NOT refactor unrelated code while fixing an error.
- Do NOT weaken `#![forbid(unsafe_code)]` outside platform/arch/direct_boot.
- Do NOT proceed past the ISO test into hardware steps without explicit user
  approval naming the device.
