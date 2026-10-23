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
