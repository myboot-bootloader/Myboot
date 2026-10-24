# MyBoot TESTLOG

Append one entry per test/fix run: date, command, exact result (paste key serial
lines), and any PROVEN/UNPROVEN status change. See CLAUDE.md §6.

## 2026-09-09 — added install.ps1 (Windows counterpart to install.sh)

User reported the one-line installer fails for Windows users, since
`curl ... | sudo sh` assumes POSIX `sh`/`sudo`/`efibootmgr`, none of which exist on
Windows. Added `install.ps1`: self-elevates via UAC (no `sudo`), finds the ESP with
`Get-Partition` (GptType `{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}`, the UEFI-spec ESP
GUID), mounts it to a spare drive letter only if not already mounted, downloads +
verifies the same GitHub release asset (`myboot.efi` + `.sha256`) as `install.sh`,
places `\EFI\MyBoot\BOOTX64.EFI` + default `config.toml`, and registers a firmware
boot entry with `bcdedit /copy {bootmgr}` + `bcdedit /set {fwbootmgr} displayorder`
(the Windows equivalent of `efibootmgr --create`; confirmed idempotent the same way
install.sh is — checks `bcdedit /enum firmware` for an existing "MyBoot" description
before creating another). Same env-var knobs as install.sh
(`MYBOOT_REPO`/`MYBOOT_TAG`/`MYBOOT_ESP`/`MYBOOT_NO_REGISTER`/`MYBOOT_MAKE_DEFAULT`).

This is installer tooling only — no Rust/`no_std` core code touched, no change to
the UEFI target build or the boot transaction logic. Not yet run on a real Windows
machine (I have no Windows host in this sandbox); NOT run against real hardware —
per CLAUDE.md §0.4, that step is the user's to do and confirm, on a machine and ESP
they name explicitly. Documented in README.md, INSTALL_GUIDE.md (new "Windows"
section), and DISTRIBUTION_GUIDE.md.

## 2026-10-23 — install.ps1: fix UEFI preflight false negative on ordinary Windows

Evidence: docs/bug_reports/MyBoot_Windows_UEFI_Installer_Bug_Report.md — on a real
Windows UEFI machine the installer aborted with
`not a UEFI system (PEFirmwareType=); MyBoot is UEFI-only.` The check read
`HKLM:\SYSTEM\CurrentControlSet\Control\PEFirmwareType`, a value that is only
present under WinPE, so on a normal install it was empty and the check always failed.

Change (install.ps1, preflight step 2, 7 insertions / 3 deletions): replaced the registry
read with `(Get-ComputerInfo -Property BiosFirmwareType).BiosFirmwareType -eq 'Uefi'`
(wraps the Win32 GetFirmwareType() API, works on a full Windows install; needs
PowerShell 5.1, which ships with Windows 10/11). The Die message now reports
BiosFirmwareType. No fallback added: the evidence does not require one.

Ran (Linux, pwsh via nix shell, nothing installed system-wide, no disk/ESP/NVRAM touched):
- PowerShell parser on install.ps1: 0 errors.
- Pester, 21 tests with bcdedit / network / Get-ComputerInfo mocked: 21 passed, 0 failed.
  They reproduce the false negative on the old check, show the new check accepts UEFI and
  rejects BIOS, and cover file placement, re-run/uninstall, checksum-mismatch abort, and
  bcdedit GUID parsing in English/German/Japanese output.

Status: detection fix tested with mocks; real-machine boot NOT observed. Still UNPROVEN:
firmware actually booting the registered entry, an ESP without a drive letter, Secure Boot
on, and a real Windows UEFI run of the fixed script. Per CLAUDE.md §0.4 that step is the
user's to run on a machine they name (a dry run with MYBOOT_NO_REGISTER=1 against a temp
ESP is the safe first step).

## 2026-10-24 — PROPOSAL (not implemented): Challenge 1, boot-time scanning overhead

Branch: `feat/challenge-1`. No code changed yet. Per CLAUDE.md §6 this is a behaviour
change (it persists derived state), so it needs review before implementation.

### Challenge as stated
MyBoot rescans every volume, ESP and NixOS generation on each boot. Proposed fix: keep a
hash of the "hardware signature" in NVRAM. If it matches, skip the crawl and load the graph
from cache. A key at boot forces a fresh scan.

### What the code does today (read 2026-10-24)
Pipeline (`crates/engine/src/pipeline.rs`), before the menu can draw:
1. `MultiVolume::discover()` calls `connect_controller(recursive)` on EVERY handle, then
   lists all SimpleFileSystem handles (`platform/src/volume/mod.rs`).
2. Per-volume diagnostics (pipeline.rs:51): `loader_string_sample` reads the WHOLE
   `\EFI\BOOT\BOOTX64.EFI` into memory on each volume, only to log it.
3. `discover_multi` (pipeline.rs:56) reads `Boot####` variables once, then runs
   `scan_volume` on each volume: esp → distros → bls → bootspec → refine.
4. `health::assess_all` (pipeline.rs:59) calls `exists` for each entry's loader, kernel
   and initrd through `MultiVolume`, which tries every volume in turn.

Measured nothing yet. Where the cost is, from reading the code:
- **Whole-file reads.** `FileStore::read` reads an entire file.
  - `refine.rs:24` reads the WHOLE kernel of every BLS Linux entry just to check the PE
    header. The comment says "a small read is enough", but the code doesn't do a small
    read. That's roughly 10–15 MB per kernel. This scales with "hundreds of kernels" more
    than anything else.
  - `esp.rs:104` (`media_name`) reads whole loader binaries (BOOTX64/grubx64/shimx64)
    on each volume to search for distro strings, and step 2 reads BOOTX64 a second time.
- **Reopening the volume root.** `Volume::open_root` (volume/mod.rs:34) opens
  SimpleFileSystem exclusively and re-runs `open_volume` on EVERY read, exists and
  list_dir call.
- **Health checks every volume.** `MultiVolume::exists` (volume/mod.rs:249) tries every
  volume for every path. It ignores `entry.volume`, so N entries × M volumes. Side
  finding: an entry can be judged healthy because the file exists on a *different*
  volume.
- **Volume id recomputed.** `chainload_on` (volume/mod.rs:220) rehashes the device path
  of every volume for each attempt. This is minor.
- Partition tables are **never read at boot**. The `storage::gpt` parser has no caller in
  engine, platform or discovery, and nothing uses BlockIO. A "partition-table hash" would
  mean new BlockIO code in `platform`.

### Problems with the challenge's design as written
1. **A partition-table hash alone gives wrong results.** A new NixOS generation, kernel
   update, BLS entry or a deleted loader changes FILES, not partitions. A hit on an
   unchanged partition table would show entries that no longer exist, or hide new ones.
2. **The graph won't fit in NVRAM.** Variable storage is small (tens of KB in total on
   many firmwares) and wears with writes. Hundreds of entries won't fit, and writing it
   every time it changes wears the flash.
3. **It conflicts with ADR 0001 / §3.9.** `persistence/src/lib.rs:5` and
   `graph/src/lib.rs:6` both say the Boot Graph is "derived, never persisted". A cache
   means changing that decision explicitly, not quietly working around it.
4. **The force-rescan key can't be read in time.** Keys are only polled inside
   `present()`, which runs after discovery.

### Proposed plan (two phases; phase A is reviewable on its own)

**Phase A: make the full scan cheap. No new persisted state.**
- A0. Measure first. Log timestamps around stages 1–3 (volume connect, discovery,
  health), add a QEMU fixture ESP with e.g. 3 volumes × 100 NixOS generations + 20 BLS
  kernels, and record before/after numbers here.
- A1. Add `FileStore::read_head(path, max) -> IoResult<Vec<u8>>`: a trait default that
  falls back to `read`, overridden in `platform` with a bounded `File::read`. Use it in
  `refine.rs` (PE header, a few KB) and `media_name` (string search over a bounded
  prefix). Pure-crate tests go through the mock.
- A2. Remove or gate the whole-file read for the `loader_string_sample` diagnostic
  (pipeline.rs:51). Behaviour change: it's diagnostics only, so I'd like your call
  between deleting it and keeping it on a bounded read.
- A3. Health checks the entry's OWN volume when `entry.volume` is set, instead of
  `MultiVolume::exists` across all volumes. This is faster and also fixes the
  cross-volume false "Healthy".
- A4. Compute `Volume::id()` once when the `Volume` is created.

Phase A may well be enough. It removes the costs that grow with kernel count without
adding any way to serve a stale menu.

**Phase B: a fingerprinted discovery cache. Only if A0 numbers still justify it.**
- Fingerprint (pure, `no_std`, host-tested). A hash over: the sorted volume ids (device
  paths), and for each volume the names + sizes + modification times (FileInfo
  metadata, no file contents) of exactly the paths discovery looks at: `\EFI`, each
  `\EFI\<vendor>`, `\EFI\BOOT`, `\loader\entries`, `\EFI\nixos` and its generation dirs.
  This is still directory listing, but it is far cheaper than parsing and full reads.
  `Boot####` variables are always re-read live, because they're cheap and change outside
  the ESP.
- Storage split, to respect NVRAM limits:
  - NVRAM (vendor GUID): one small variable `GraphCacheKey`, holding a format version
    and a 32-byte fingerprint. Written only when the fingerprint changes.
  - ESP: `\EFI\MyBoot\graph.cache` on MyBoot's OWN volume, holding a versioned,
    length-checked encoding of the discovered `RawEntry` list (from before health
    checks). Decoding never panics; any parse failure means a full scan.
- Health checks always run live over the cached entries. They're cheap after A3, and
  they guarantee a missing file still marks the entry Unbootable.
- New code, one job per file: `discovery/src/fingerprint.rs` (pure),
  `persistence/src/graph_cache.rs` (pure encode/decode + key variable),
  `engine/src/scan_cache.rs` (hit/miss decision and writing). `platform` gains only the
  metadata listing it needs.
- Invalidation and safety:
  - Fingerprint mismatch, missing or corrupt cache, or a version change → full scan,
    then rewrite the cache.
  - An unconfirmed boot marker from the previous boot → full scan. Ties into
    commit/rollback.
  - A cached entry that fails to hand off → full rescan before moving to the next
    candidate, so the never-dead-end guarantee still uses up-to-date information.
  - Force rescan: a non-blocking `read_key` check before discovery (e.g. a pending
    `R`/F5 press), plus `cache = false` in `config.toml`, plus a host-CLI
    `myboot rescan` that deletes `GraphCacheKey`.
- Hash: a cryptographic hash isn't needed for security here. Anyone who can write the
  ESP can already replace the loaders, and Secure Boot still checks every image at
  LoadImage. Options are SHA-256 via the `sha2` crate (no_std, but a new dependency) or
  a 128-bit non-cryptographic hash written in-crate. Your decision.
- Needs: an ADR 0001 / §3.9 amendment recording that tier 2 may hold a *derived,
  disposable* discovery cache.

### Decisions needed from the reviewer
1. Do Phase A first, then re-measure before committing to Phase B? (Recommended.)
2. A2: delete the `loader_string_sample` diagnostic, or keep it on a bounded read?
3. Phase B: cache file on the ESP plus fingerprint in NVRAM (recommended), or something
   else?
4. Hash choice: `sha2` dependency, or an in-crate non-cryptographic hash?
5. Force-rescan trigger: which key(s), and is the host-CLI `myboot rescan` wanted?

Status: no change to PROVEN/UNPROVEN. Nothing built or run for this entry.

## 2026-10-24 — Challenge 1, Phase A0: stage timing + NixOS-shaped QEMU fixture

Reviewer decisions (2026-10-24): Phase A only, then stop at the gate. Remove the
`loader_string_sample` diagnostic (A4). Skip A7 (connect narrowing). Skip A8 (BootOrder-only
scan, which needs measuring first). Do not commit. The implementation guide this follows
was written against another checkout (`085c650`). Line references were re-checked here.

Changes:
- `crates/arch/src/x86_64/tsc.rs` (new): `read_tsc()` via `core::arch::x86_64::_rdtsc`.
  `unsafe` is confined to `arch` (CLAUDE.md §0.7).
- `crates/engine/src/timing.rs` (new): `StageClock`, calibrated against a 10 ms
  `boot::stall`. It logs `myboot: timing <stage> <us>us total=<us>us`. It's a zero-sized
  no-op unless built with the new `engine` feature `boot-timing`, so default builds are
  unchanged.
- `crates/platform/src/volume/mod.rs`: added `MultiVolume::enumerate()` (no connect);
  `discover()` = `connect_all_controllers()` + `enumerate()`, same behaviour as before.
- `crates/engine/src/pipeline.rs`: calls connect then enumerate separately; laps at
  connect / enumerate / diagnostics / discovery / health / to-menu.
- pyboot: `Timing` event + parser rule, `report.render_timings`, `build.build_efi(features)`,
  `esp.stage(nixos_generations=, nixos_kernels=)`. The fixture holds N
  `\loader\entries\nixos-generation-N.conf` sharing K 12 MB PE kernels in `\EFI\nixos`,
  matching systemd-boot's NixOS layout. New subcommand `./manage.py bench`. Two new pytest
  cases (10 passed).

Ran:
- Host tests: 119 passed, 0 failed (unchanged). UEFI build: clean, with and without
  `--features engine/boot-timing`.
- `./manage.py smoke`: 6/6 (before the change).
- `./manage.py bench` (QEMU/OVMF, ESP = vvfat on emulated IDE; relative numbers only):

| stage | smoke ESP (median of 3) | +10 gens / 4 kernels | +100 gens / 4 kernels |
|---|---|---|---|
| connect | 171.1 ms | 171.2 ms | 171.3 ms |
| enumerate | 2.6 ms | 2.5 ms | 2.5 ms |
| diagnostics | 46.7 ms | 47.5 ms | 46.2 ms |
| discovery | 40.2 ms | 11 985.7 ms | **134 954.9 ms** |
| health | 2.5 ms | 2.8 ms | 5.7 ms |
| to-menu (total) | 293.1 ms | 12 278.4 ms | **135 669.5 ms** |
| entries found | 2 | 12 | 102 |

Discovery costs about 1.3 s per BLS generation here. That is the whole-kernel read in
`refine.rs:24` (12 MB × each entry, shared kernels re-read). The diagnostics stage
(about 46 ms) is mostly the whole read of `\EFI\BOOT\BOOTX64.EFI` (A4).

Status: no change to PROVEN/UNPROVEN.

## 2026-10-24 — Challenge 1, Phase A (A1–A6, A9): remove boot-time scan waste

Not committed (reviewer's choice). A7 and A8 were skipped by decision; see the A0 entry.

Changes, file by file. uefi 0.33 facts were checked in the 0.33.0 crate source:
- **A1** `crates/ports/src/meta.rs` (new: `DirEntryMeta`). `FileStore` gains
  `read_head(path, max_len)` and `list_dir_meta(path)`. These are required methods with no
  default, so nothing silently falls back to whole-file reads. Implemented in
  `platform/src/fs/mod.rs` (`read_head_in`: bounded `RegularFile::read` loop;
  `list_dir_meta_in`: `FileInfo::{file_name, file_size, modification_time, is_directory}`,
  `Time::{year..second}`; `list_dir_in` now delegates to it, same names as before),
  on `Volume`/`UefiFileStore`/`MultiVolume` (first volume wins, like `read`/`list_dir`),
  and on the mock (`with_mtime`).
- **A2** `discovery/src/refine.rs`. It reads only the PE header: 4 KiB first, and a second
  read up to `e_lfanew + 94` bytes, capped at 64 KiB. Each distinct kernel path is
  classified once. Before, it read the whole kernel for every entry (the comment there
  claimed "a small read"). Deviation from the guide: a header past 64 KiB keeps the
  EFI-stub assumption instead of being sent to direct boot. Firmware `LoadImage` still
  verifies the image, so this opens no new route into direct boot.
- **A3** `discovery/src/bootspec.rs`. It only probes for `bootspec.json`/`boot.json` inside
  *directories* of `\EFI\nixos`. Kernel/initrd files there used to cost two failed opens
  each.
- **A4** Removed `esp::loader_string_sample` and its call/log line in `pipeline.rs`. It was
  diagnostics only: a whole-file read of `\EFI\BOOT\BOOTX64.EFI` on each volume, with no
  consumer in code, harness or docs. Side effect: `media_name`'s doc comment, which had
  been stranded above the removed function, is attached to `media_name` again.
- **A5** `health/src/lib.rs`: new `assess_all_scoped(graph, resolve)`, and `assess_entry`
  accepts `?Sized`. `pipeline.rs` checks each entry against **its own volume**.
  Firmware-variable entries (no volume) still check every volume. This fixes a real bug,
  reproduced first with a host test: a disk-A entry whose kernel exists only on disk B was
  `Healthy`. Launch made it worse, not better: `chainload_on` (`volume/mod.rs`) falls
  through to "first volume that has the path", so that entry would have booted **disk B's
  kernel with disk A's cmdline/initrd**. Now it is `Unbootable`, and the transaction never
  selects it.
- **A6** `platform/src/volume/mod.rs`: `Volume { handle, id }`. The id is computed once at
  enumeration by `compute_id` (same hash body, moved). `id()` returns `&str`. Ids are
  byte-identical, confirmed in QEMU: `vol0c550065d826754e` before and after. This matters
  because `LastGood` stores ids ending in `@vol…`.
- **A9** `platform/src/fs/mod.rs` `write_in`: a non-append write now deletes an existing
  *regular file* first (`File::delete`), so a shorter write no longer leaves the old tail.
  A directory at the path still returns the "path is a directory" error. The guide's
  version would have deleted it. Nothing in the boot path writes files today, so this is
  compiled but **not exercised** by any run.

Ran:
- Host tests: **127 passed, 0 failed** (119 before; 8 new: 2 ports, 3 refine, 1 bootspec,
  2 health). `python -m pytest tools/tests`: 10 passed. UEFI build clean, with and
  without `engine/boot-timing`. No `#![forbid(unsafe_code)]` line touched.
- `./manage.py smoke`: **6/6** (default build).
- `./manage.py bench` (QEMU/OVMF, vvfat ESP; relative numbers only):

| stage | smoke ESP before → after | 10 gens before → after | 100 gens before → after |
|---|---|---|---|
| connect | 171.1 → 169.6 ms | 171.2 → 170.9 ms | 171.3 → 170.2 ms |
| diagnostics | 46.7 → 7.6 ms | 47.5 → 7.7 ms | 46.2 → 7.7 ms |
| discovery | 40.2 → 40.0 ms | 11 985.7 → 114.2 ms | **134 954.9 → 263.2 ms** |
| health | 2.5 → 2.5 ms | 2.8 → 2.6 ms | 5.7 → 3.3 ms |
| to-menu (total) | 293.1 → 251.7 ms | 12 278.4 → 365.5 ms | **135 669.5 → 874.2 ms** |

  "After" values are the median of 3 runs. The 100-generation "before" is 1 run (each took
  about 2¼ minutes).
- The discovered entry set is unchanged at 100 gens: a diff of the sorted
  `discovered …` + `'<title>' loader=… health=…` lines from before and after is empty
  (102 entries, 102 bootable).

Gate (guide §5), as far as QEMU can say: at 100 generations, discovery is now 263 ms, about
30% of time-to-menu. In QEMU the largest fixed cost is now `connect` (≈170 ms, untouched,
A7 skipped). The rest of `to-menu` at 100 gens (≈428 ms after health) grows with entry
count. It includes printing one serial line per entry, which this run did not separate
out. Whether Phase B is needed is a **real-hardware** question (A0 build from a removable
USB the user names).

Status: no change to PROVEN/UNPROVEN. The ISO test (CLAUDE.md §3.4) was not re-run. A6
touches the volume ids that `chainload_on` matches on, and the smoke handoff exercised that
path. The device-path load itself is unchanged.

## 2026-10-24 — ISO test (CLAUDE.md §3.4): real OS loaders boot via device-path chainload

Build: the Phase A working tree (default features). ISOs supplied by the user:
`ubuntu-26.04.1-live-server-amd64.iso`, `Parrot-home-7.3_amd64.iso`. Both were attached
read-only as separate virtio-scsi CD volumes beside the minimal MyBoot ESP (same QEMU
command as `pyboot/qemu.py`). The menu was driven through the QEMU monitor (`sendkey`),
and screens captured with `screendump`.

Serial (both runs):
```
myboot: scanning 3 volume(s)
myboot: vol vole1bb89509c762655 label='ESP' \EFI\BOOT=[grubx64.efi,mmx64.efi,bootx64.efi]
myboot: vol volcca9ea4d76bb1ee6 label='' \EFI\BOOT=[bootx64.efi,grubx64.efi,mmx64.efi]
myboot: self-excluded own loader on vol vol0c550065d826754e
myboot: discovered 2 entr(y/ies), 2 bootable
myboot:   'Debian' loader=Some("\\EFI\\BOOT\\BOOTX64.EFI") health=Healthy
myboot:   'Ubuntu' loader=Some("\\EFI\\BOOT\\BOOTX64.EFI") health=Healthy
```
- **Parrot** (policy default, no keys pressed):
  `handing off to 'efi:fallback:/efi/boot/bootx64.efi@volcca9ea4d76bb1ee6'`.
  - Screen: Parrot's own themed GRUB, "Parrot home Live Boot Menu - 7.3 amd64".
  - After `sendkey ret` on "Try / Install": Parrot boot splash, then the **Parrot live
    desktop login ("Parrot Live user")** at about 108 s.
- **Ubuntu** (`sendkey down`, `sendkey ret` in MyBoot's menu):
  `handing off to 'efi:fallback:/efi/boot/bootx64.efi@vole1bb89509c762655'`.
  - Screen at about 198 s: the **Ubuntu Subiquity installer** language selection
    ("Willkommen! Bienvenue! Welcome!...").
- In both cases the ISO's loader found its own GRUB config, theme, kernel and initrd on its
  own volume. That is the case CLAUDE.md §1 listed as UNPROVEN.

Harness fix: `pyboot/iso.py` headless mode stopped on `discovered \d+ entr`. pexpect stops
capturing at the end of the match, so the line was cut before `…, N bootable`. `logparse`
then could not parse it, and `discovered_bootable` failed even though discovery had
succeeded (serial above). The stop pattern now matches the whole line. Before:
`./manage.py iso-test --no-ui <both ISOs>` gave 1/2. After: **2/2**. This bug predates
Phase A.

Observed, not changed: the Parrot ISO is titled "Debian", not "Parrot OS". The unmodified
`HEAD` build (1f53b33), built in a separate worktree and run against the same ISOs, shows
the same "Debian" title. So it is existing `media_name` behaviour, not a Phase A
regression.

Status change: **chainloading a real OS loader that must find its own files, via the
device-path path, is now PROVEN on QEMU/OVMF** (Ubuntu 26.04.1 server ISO to the installer;
Parrot 7.3 ISO to the live desktop). Still UNPROVEN: real hardware, an installed (on-disk)
OS such as Windows Boot Manager or an installed shim/GRUB, and Secure Boot enabled.

## 2026-10-24 — PROPOSAL (not implemented): harness line truncation + Parrot ISO titled "Debian"

### 1. Harness: stop markers cut the matched line short
Cause: `pyboot/qemu.py` `run()` stops at the END OF THE REGEX MATCH. Anything after it on
the same line is never captured. This is a property of the capture loop, not of one
pattern. Affected markers:
- `iso.py` `discovered \d+ entr`. Patched today to match the whole line; it gave 1/2 on a
  working run.
- `smoke.py` + `iso.py` `no candidate could be booted`. `logparse` needs `(N tried)`, so a
  dead end never yields `NoCandidate`. Smoke still FAILS in that case, but reports
  "last handoff failed" instead of "dead-ended after N tries". `no_dead_end` would
  wrongly pass if a test used it.

Plan:
- In `qemu.py`, factor the read loop into `_capture(child, markers)`. After a marker
  matches, read on to the end of that line (`expect([r"\r?\n", EOF, TIMEOUT], timeout=2)`)
  before stopping. No marker can truncate a line again. Keep today's whole-line pattern in
  `iso.py`.
- pytest, no QEMU: drive `_capture` with `pexpect.spawn("sh", ["-c", "printf …; sleep 5"])`.
  - A marker mid-line gives the full line.
  - `no candidate could be booted (3 tried)` parses as `NoCandidate(3)`.
  - A marker with no trailing newline returns within the 2 s grace period and doesn't hang.
- Re-run `./manage.py smoke` and `./manage.py iso-test --no-ui <both ISOs>`.

### 2. Parrot 7.3 ISO titled "Debian"
Evidence: the ISO's EFI FAT partition (sector 1916) was read with mtools.
- It has no `BOOT.CSV` and no `.disk/info`. `\boot\grub\grub.cfg` is a 104-byte redirect
  (`search --file /.disk/info` / `configfile`) with no distro marker. So `media_name`
  reaches step 4, the loader-binary scan (`esp.rs`).
- Step 4 is FILE-first and returns on the first file with ANY marker. It scans
  `BOOTX64.EFI` first, and on Parrot that is **Debian's signed shim**, reused unchanged:
  SBAT `shim.debian,1,Debian,shim,15.8`; markers: debian ×7, nothing else. So "Debian"
  is returned before `grubx64.efi` (parrot ×9; SBAT `grub.parrot,1,Parrot Security,…`)
  is ever read. The table's "parrot before debian" order only applies within one file.
- Ubuntu 26.04.1 works by luck of the same order: its shim is Ubuntu-signed
  (`shim.ubuntu`, ubuntu ×5).
- Existing behaviour: the unmodified HEAD build shows the same title.

Options:
- **A (minimal, recommended first):** in step 4, scan `grubx64.efi` before `BOOTX64.EFI`,
  then `shimx64.efi`. GRUB is built by the distro; shim often identifies only the signer.
  It's a one-line reorder. Checked against the real binaries with the same substring
  rules: Parrot grub → "Parrot OS"; Ubuntu grub (ubuntu ×10, debian ×2, ubuntu ranks
  first) → "Ubuntu". Tests: a host test with a Debian-only shim and a parrot-bearing grub
  expects "Parrot OS". Existing esp tests keep passing.
- **B (robust, larger):** identify the distro from SBAT. Add a bounded, total `.sbat`
  section lookup in `storage::pe` (section-table walk, Kani-provable like the other
  parsers) and an SBAT CSV parser in discovery. Take the last `grub.<x>` / `shim.<x>` line
  whose `<x>` is in the marker table. That skips non-distro components such as
  `grub.peimage` and keeps the most downstream vendor (Parrot:
  debian → debian14 → **parrot**). Check GRUB before shim, then fall back to the substring
  scan. This avoids substring false positives: e.g. the marker "tails" matches any binary
  containing "details". Needs a new module and more tests, and is a bigger behaviour
  change.
- Either way, verify in QEMU with both ISOs (titles "Parrot OS" / "Ubuntu"), smoke 6/6,
  and the host suite green.
- Not addressed (noted only): step 4 still reads whole loader binaries (2–3 MB) on
  volumes without a better name source. B could later use a bounded read once a ranged
  `FileStore` read exists.

## 2026-10-24 — Fix: harness stop markers no longer truncate the matched line (branch feat/bug-fix)

Implements part 1 of the proposal above. `pyboot/qemu.py`: the read loop is now
`_capture(child, markers)`. After a marker matches, it reads the rest of that line
(`expect([r"\r?\n", EOF, TIMEOUT], timeout=LINE_GRACE_SECS=2)`). So
`no candidate could be booted (N tried)` and any future marker line reach `logparse`
whole. `run()` behaves the same otherwise.

New `tools/tests/test_qemu_capture.py` (4 tests, driven by `sh -c printf …`, no QEMU):
- a marker mid-line gives the whole line;
- the dead-end line parses as `NoCandidate(3)`;
- a marker with no line ending returns within the grace period;
- a timeout is reported.

Mutation check: with the end-of-line read removed (old behaviour), 3 of the 4 fail. With
it restored, all pass.

Ran:
- pytest 14 passed (was 10).
- `./manage.py smoke` 6/6.
- `./manage.py iso-test --no-ui <Ubuntu 26.04.1, Parrot 7.3>` 2/2.

## 2026-10-24 — Fix: name live ISOs from SBAT metadata (Parrot no longer "Debian") (branch feat/bug-fix)

Implements option B of the proposal above, approved by the reviewer.

Changes:
- `crates/storage/src/pe/section.rs` (new): `section::find(buf, name)`, a total PE/COFF
  section-table walk using only the checked `bytes` helpers. Sections are capped at the
  PE/COFF limit of 96. The length is `VirtualSize` when smaller than the padded
  `SizeOfRawData`. Also added to the storage "no input panics" sweep. New Kani harness
  `pe_section_find_is_total` in `proofs.rs`. It is **written but not run**: cargo-kani is
  not installed here.
- `crates/discovery/src/sbat.rs` (new): `distro_from_sbat(section)`. It parses the SBAT CSV
  and takes the LAST `<component>.<vendor>` whose vendor EXACTLY matches the distro table
  (`esp::DISTRO_MARKERS`, now `pub(crate)`). It skips non-distro suffixes
  (`grub.peimage`) and unknown ones (`grub.debian14`). There is no substring matching.
- `crates/discovery/src/esp.rs` `media_name` step 4:
  - Loaders are read once each, in order `grubx64.efi` → `BOOTX64.EFI` → `shimx64.efi`
    (was BOOTX64 first).
  - SBAT is tried on each first. Only if none names a known distro does it fall back to
    the old marker substring scan, in the same order.
  - Steps 1–3 (BOOT.CSV, .disk/info, grub.cfg) and step 5 (label) are unchanged.

Tests (10 new):
- storage 2: named section + VirtualSize trim + absent/prefix name; malformed and
  truncated images, too many sections, far pointer.
- sbat 5: last vendor wins; peimage skipped; Debian shim; NUL padding / upstream-only;
  malformed input and "details".
- esp 3: Parrot layout (Debian shim + parrot GRUB) gives "Parrot OS"; Ubuntu layout gives
  "Ubuntu"; SBAT beats the "details"⊃"tails" false positive.
- Mutation check: with the old step 4 (BOOTX64 first, substring only), the Parrot and
  "details" tests fail. The Ubuntu test passes, as it did before.

Ran:
- Host tests **137 passed, 0 failed** (was 127). pytest 14 passed. UEFI build clean.
- `./manage.py smoke` 6/6.
- `./manage.py iso-test --no-ui <Ubuntu 26.04.1, Parrot 7.3>`: 2/2 on 4 of 5 runs. The
  first run after the build scored 0/2. Its output was not kept, so the cause is unknown.
  Two stray QEMU processes from earlier manual monitor tests were still running at the
  time (killed). The 3 runs after that all passed.
- Serial with both ISOs:
  ```
  myboot: discovered 2 entr(y/ies), 2 bootable
  myboot:   'Parrot OS' loader=Some("\\EFI\\BOOT\\BOOTX64.EFI") health=Healthy
  myboot:   'Ubuntu' loader=Some("\\EFI\\BOOT\\BOOTX64.EFI") health=Healthy
  ```
  The MyBoot menu screenshot shows "Parrot OS" and "Ubuntu". The default handoff still
  reaches Parrot's GRUB.

Observed, not changed: the staged ESP's config writes `timeout_secs = 20`, but
`config::schema` reads the key `timeout`, so the ISO menu auto-boots after the default 5 s
(the screenshot shows "auto-boot 1s"). The key names in `pyboot/esp.py` and the schema
disagree.
