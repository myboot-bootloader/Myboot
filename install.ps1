#Requires -Version 5.1
<#
MyBoot one-line installer for Windows.

    irm https://raw.githubusercontent.com/ctrl-routing-Mosesgarlic/Myboot/main/install.ps1 | iex

Downloads the latest released myboot.efi from GitHub, verifies its SHA-256,
installs it to <ESP>\EFI\MyBoot\BOOTX64.EFI, writes a default config (only if
none exists), and registers a UEFI firmware boot entry via bcdedit. It NEVER
formats a disk or removes another OS's loader -- it only writes \EFI\MyBoot and
adds one firmware boot entry. This is the Windows counterpart to install.sh
(same env-var knobs, same "coexist, never destroy" contract).

Design notes vs. a naive script:
  * All bcdedit interaction is LOCALE-INDEPENDENT. We never parse translated
    English strings; we extract GUIDs by their universal format and detect our
    own entry via a GUID we persist next to the loader.
  * Re-runs are idempotent: the stored GUID is reused (device/path refreshed) if
    it still exists, otherwise a fresh entry is created.
  * Secure Boot is checked and the user is warned (MyBoot is not signed yet).

Options via environment variables (same names as install.sh):
    MYBOOT_REPO            override the source repo (default below)
    MYBOOT_TAG             install a specific release (default: latest)
    MYBOOT_ESP             override ESP auto-detection, e.g. "S:"
    MYBOOT_NO_REGISTER=1   skip the bcdedit entry (just place the files)
    MYBOOT_MAKE_DEFAULT=1  put MyBoot first in the firmware display order
    MYBOOT_UNINSTALL=1     remove \EFI\MyBoot and the MyBoot firmware entry
#>

[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$InstallScriptUrl = 'https://raw.githubusercontent.com/ctrl-routing-Mosesgarlic/Myboot/main/install.ps1'
$Repo    = if ($env:MYBOOT_REPO) { $env:MYBOOT_REPO } else { 'ctrl-routing-Mosesgarlic/Myboot' }
$Tag     = if ($env:MYBOOT_TAG)  { $env:MYBOOT_TAG }  else { 'latest' }
$Label   = 'MyBoot'
$SubDir  = 'EFI\MyBoot'
$UA      = @{ 'User-Agent' = 'myboot-install.ps1' }
$GuidRx  = '\{[0-9a-fA-F]{8}-([0-9a-fA-F]{4}-){3}[0-9a-fA-F]{12}\}'

function Say  ($m) { Write-Host "[myboot] $m" -ForegroundColor Cyan }
function Ok   ($m) { Write-Host "[ ok ]   $m" -ForegroundColor Green }
function Warn ($m) { Write-Host "[warn]   $m" -ForegroundColor Yellow }
function Die  ($m) { Write-Host "[fail]   $m" -ForegroundColor Red; exit 1 }

# --- 1. elevate: self-relaunch with a UAC prompt if we aren't admin -----------
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$isAdmin  = ([Security.Principal.WindowsPrincipal]$identity).IsInRole(
              [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Say 'not elevated; requesting Administrator (UAC prompt)...'
    $psArgs = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File')
    if ($PSCommandPath) {
        Start-Process powershell.exe -Verb RunAs -ArgumentList ($psArgs + "`"$PSCommandPath`"")
    } else {
        # Invoked as `irm ... | iex`: there is no file on disk. Re-fetch this same
        # script, forward the env-var knobs, write it WITHOUT a BOM (a BOM breaks
        # `powershell -File`), and relaunch elevated from the temp file.
        $prologue = foreach ($n in 'MYBOOT_REPO','MYBOOT_TAG','MYBOOT_ESP','MYBOOT_NO_REGISTER','MYBOOT_MAKE_DEFAULT','MYBOOT_UNINSTALL') {
            $v = [Environment]::GetEnvironmentVariable($n)
            if ($v) { "`$env:$n = '$v'" }
        }
        $body = Invoke-RestMethod -Uri $InstallScriptUrl -Headers $UA -UseBasicParsing
        $text = (($prologue + $body) -join "`r`n")
        $tmp  = Join-Path $env:TEMP 'myboot-install.ps1'
        [IO.File]::WriteAllText($tmp, $text, (New-Object Text.UTF8Encoding($false)))
        Start-Process powershell.exe -Verb RunAs -ArgumentList ($psArgs + "`"$tmp`"")
    }
    exit 0
}

# --- 2. preflight: UEFI + Secure Boot -----------------------------------------
# Get-ComputerInfo wraps the Win32 GetFirmwareType() API (Windows 8+), which works on a
# full Windows install. The PEFirmwareType registry value is documented only for WinPE
# and is absent on normal Windows, so it must not be used here.
$firmwareType = (Get-ComputerInfo -Property BiosFirmwareType).BiosFirmwareType
if ($firmwareType -ne 'Uefi') {
    Die "not a UEFI system (BiosFirmwareType=$firmwareType); MyBoot is UEFI-only."
}

try {
    if (Confirm-SecureBootUEFI) {
        Warn 'Secure Boot is ENABLED. MyBoot is not signed yet, so firmware will refuse to run it.'
        Warn 'Disable Secure Boot in your firmware settings (or enroll MyBoot via MokManager) before booting it.'
    }
} catch { }   # Confirm-SecureBootUEFI throws on firmware that lacks the variable; ignore.

# --- 3. locate the ESP (mount a temp letter if it has none) -------------------
$Esp = $env:MYBOOT_ESP
$espPartition = $null
$mountedAccessPath = $null
if (-not $Esp) {
    # {c12a7328-f81f-11d2-ba4b-00a0c93ec93b} is the UEFI EFI System Partition GUID.
    $espPartition = Get-Partition -ErrorAction SilentlyContinue |
        Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | Select-Object -First 1
    if (-not $espPartition) { Die 'could not find the EFI System Partition; set $env:MYBOOT_ESP = "X:" and re-run.' }

    $existing = $espPartition.AccessPaths | Where-Object { $_ -match '^[A-Za-z]:\\$' } | Select-Object -First 1
    if ($existing) {
        $Esp = $existing.TrimEnd('\')
    } else {
        $letter = 68..90 | ForEach-Object { [char]$_ } | Where-Object { -not (Test-Path "${_}:\") } | Select-Object -First 1
        if (-not $letter) { Die 'no free drive letter to mount the ESP.' }
        $mountedAccessPath = "${letter}:\"
        Add-PartitionAccessPath -DiskNumber $espPartition.DiskNumber -PartitionNumber $espPartition.PartitionNumber -AccessPath $mountedAccessPath
        $Esp = "${letter}:"
        Say "mounted the ESP at $Esp (will unmount when done)"
    }
}
$Esp  = $Esp.TrimEnd('\')
$dest = Join-Path "$Esp\" $SubDir
Say "using ESP: $Esp"

try {
    # --- 4. uninstall path ----------------------------------------------------
    if ($env:MYBOOT_UNINSTALL -eq '1') {
        $markerPath = Join-Path $dest '.firmware-entry'
        if (Test-Path $markerPath) {
            $guid = (Get-Content $markerPath -Raw).Trim()
            if ($guid -match $GuidRx) { & bcdedit /delete $guid /f | Out-Null; Ok "removed firmware entry $guid" }
        }
        if (Test-Path $dest) { Remove-Item $dest -Recurse -Force; Ok "removed $dest" }
        Ok 'MyBoot uninstalled. Your other loaders were never touched.'
        return
    }

    # --- 5. resolve the release asset -----------------------------------------
    $apiBase = "https://api.github.com/repos/$Repo/releases"
    $metaUrl = if ($Tag -eq 'latest') { "$apiBase/latest" } else { "$apiBase/tags/$Tag" }
    Say "resolving myboot.efi from $Repo ($Tag)"
    try { $release = Invoke-RestMethod -Uri $metaUrl -Headers $UA -UseBasicParsing }
    catch { Die "could not reach GitHub API for $Repo. $($_.Exception.Message)" }
    $efiAsset = $release.assets | Where-Object { $_.name -eq 'myboot.efi' } | Select-Object -First 1
    $shaAsset = $release.assets | Where-Object { $_.name -eq 'myboot.efi.sha256' } | Select-Object -First 1
    if (-not $efiAsset) { Die "release '$Tag' has no myboot.efi asset." }

    # --- 6. download + verify --------------------------------------------------
    $tmpDir = Join-Path $env:TEMP ([Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $tmpDir | Out-Null
    try {
        $efiTmp = Join-Path $tmpDir 'myboot.efi'
        Say 'downloading myboot.efi'
        Invoke-WebRequest -Uri $efiAsset.browser_download_url -OutFile $efiTmp -Headers $UA -UseBasicParsing

        if ($shaAsset) {
            $shaTmp = Join-Path $tmpDir 'myboot.efi.sha256'
            Invoke-WebRequest -Uri $shaAsset.browser_download_url -OutFile $shaTmp -Headers $UA -UseBasicParsing
            $expect = ((Get-Content $shaTmp -Raw).Trim() -split '\s+')[0].ToLowerInvariant()
            $actual = (Get-FileHash -Algorithm SHA256 -Path $efiTmp).Hash.ToLowerInvariant()
            if ($expect -ne $actual) { Die "checksum mismatch (expected $expect, got $actual)." }
            Ok 'checksum verified'
        } else {
            Warn 'no checksum asset; skipping verification.'
        }

        # --- 7. place on the ESP (coexisting) ---------------------------------
        New-Item -ItemType Directory -Path $dest -Force | Out-Null
        Copy-Item -Path $efiTmp -Destination (Join-Path $dest 'BOOTX64.EFI') -Force
        $cfgPath = Join-Path $dest 'config.toml'
        if (-not (Test-Path $cfgPath)) {
            $cfg = @'
# MyBoot configuration (created by install.ps1; safe to edit).
default = "auto"
timeout_secs = 5
policy = "last-good-then-default"
confirm = true
max_tries = 3
rollback = "last-good"
'@
            [IO.File]::WriteAllText($cfgPath, $cfg, (New-Object Text.ASCIIEncoding))
            Say "created $cfgPath"
        } else {
            Say 'config exists, left unchanged'
        }
        Ok "MyBoot placed at $dest (other loaders untouched)"

        # --- 8. register the firmware entry (idempotent, locale-independent) ---
        if ($env:MYBOOT_NO_REGISTER -eq '1') {
            Say 'skipping firmware registration (MYBOOT_NO_REGISTER=1). Register manually:'
            Say "  bcdedit /copy {bootmgr} /d `"MyBoot`""
            Say "  bcdedit /set <new-guid> device partition=$Esp"
            Say "  bcdedit /set <new-guid> path \$SubDir\BOOTX64.EFI"
            Say '  bcdedit /set {fwbootmgr} displayorder <new-guid> /addlast'
        } else {
            $markerPath = Join-Path $dest '.firmware-entry'
            $firmwareEnum = (& bcdedit /enum firmware) -join "`n"
            $guid = $null

            # Reuse our previously-created entry if it still exists (GUID match is
            # locale-independent). Otherwise create a fresh one.
            if (Test-Path $markerPath) {
                $saved = (Get-Content $markerPath -Raw).Trim()
                if ($saved -match $GuidRx -and $firmwareEnum -match [regex]::Escape($saved)) { $guid = $saved }
            }

            if (-not $guid) {
                $copyOut = (& bcdedit /copy '{bootmgr}' /d $Label) 2>&1 | Out-String
                if ($LASTEXITCODE -ne 0) { Die "bcdedit /copy failed: $copyOut" }
                $m = [regex]::Match($copyOut, $GuidRx)
                if (-not $m.Success) { Die "could not parse the new entry GUID from bcdedit output." }
                $guid = $m.Value
                Set-Content -Path $markerPath -Value $guid -Encoding ascii
            }

            & bcdedit /set $guid device "partition=$Esp"        | Out-Null
            if ($LASTEXITCODE -ne 0) { Die "bcdedit /set device failed for $guid." }
            & bcdedit /set $guid path "\$SubDir\BOOTX64.EFI"    | Out-Null
            if ($LASTEXITCODE -ne 0) { Die "bcdedit /set path failed for $guid." }
            & bcdedit /set $guid description $Label             | Out-Null

            $addFlag = if ($env:MYBOOT_MAKE_DEFAULT -eq '1') { '/addfirst' } else { '/addlast' }
            & bcdedit /set '{fwbootmgr}' displayorder $guid $addFlag | Out-Null
            if ($LASTEXITCODE -ne 0) { Warn "could not set firmware display order (entry still created: $guid)." }
            Ok "registered the '$Label' firmware boot entry ($guid) on $Esp"
        }
    } finally {
        Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
    }
} finally {
    if ($mountedAccessPath -and $espPartition) {
        Remove-PartitionAccessPath -DiskNumber $espPartition.DiskNumber -PartitionNumber $espPartition.PartitionNumber -AccessPath $mountedAccessPath -ErrorAction SilentlyContinue
        Say 'unmounted the temporary ESP drive letter'
    }
}

if ($env:MYBOOT_UNINSTALL -ne '1') {
    Ok "Done. Reboot and pick 'MyBoot' from the firmware boot menu (F12/F2/Esc, varies by OEM)."
    Say 'Your existing bootloader is untouched -- keep it as a fallback until MyBoot is proven.'
}
