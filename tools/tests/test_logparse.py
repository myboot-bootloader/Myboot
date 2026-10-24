"""Unit tests for the pure log-parsing and assertion logic (no QEMU needed)."""
import os, sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))

from pyboot import logparse, assertions as A
from pyboot import events as ev

# A real capture excerpt from MyBoot on OVMF (the multi-volume + fallback run).
SAMPLE = r"""
[ INFO]: crates/engine/src/pipeline.rs@036: myboot: scanning 1 volume(s)
[ INFO]: crates/engine/src/pipeline.rs@051: myboot: fs probe \EFI\BOOT\BOOTX64.EFI exists=true
[ INFO]: crates/engine/src/pipeline.rs@055: myboot: discovered 3 entr(y/ies), 3 bootable
[ INFO]: myboot:   'Windows Boot Manager' loader=Some("\\EFI\\Microsoft\\Boot\\bootmgfw.efi") health=Healthy
[ INFO]: myboot:   'EFI Fallback Loader' loader=Some("\\EFI\\BOOT\\BOOTX64.EFI") health=Healthy
[ INFO]: myboot:   'Ubuntu 24.04 LTS' loader=Some("\\EFI\\ubuntu\\shimx64.efi") health=Healthy
[ INFO]: myboot: handing off to 'linux:default:/efi/ubuntu/shimx64.efi'
[ WARN]: myboot: handoff to 'linux:default:/efi/ubuntu/shimx64.efi' failed (Firmware(LOAD_ERROR)); trying next
[ INFO]: myboot: handing off to 'windows:default:/efi/microsoft/boot/bootmgfw.efi'
[ERROR]: myboot: no candidate could be booted (3 tried)
"""

def test_parses_all_kinds():
    events = logparse.parse(SAMPLE)
    kinds = [type(e).__name__ for e in events]
    assert "VolumeScan" in kinds
    assert "FsProbe" in kinds
    assert "Discovered" in kinds
    assert kinds.count("Entry") == 3
    assert "HandOff" in kinds
    assert "HandoffFailed" in kinds
    assert "NoCandidate" in kinds

def test_field_values():
    events = logparse.parse(SAMPLE)
    scan = next(e for e in events if isinstance(e, ev.VolumeScan))
    assert scan.count == 1
    disc = next(e for e in events if isinstance(e, ev.Discovered))
    assert disc.total == 3 and disc.bootable == 3
    ubuntu = next(e for e in events if isinstance(e, ev.Entry) and "Ubuntu" in e.title)
    assert ubuntu.loader is not None and "shimx64.efi" in ubuntu.loader
    assert ubuntu.health == "Healthy"

def test_smoke_assertions_pass():
    events = logparse.parse(SAMPLE)
    assert A.scanned_at_least(events, 1).passed
    assert A.discovered_bootable(events, 3).passed
    assert A.has_entry(events, "Ubuntu").passed
    assert A.handed_off_to(events, "ubuntu").passed
    assert A.any_handoff(events).passed

def test_dead_end_and_stuck_detection():
    events = logparse.parse(SAMPLE)
    # this sample DID dead-end, so no_dead_end should FAIL (detects it correctly)
    assert not A.no_dead_end(events).passed
    # ubuntu was handed off but bounced back -> handoff_stuck should FAIL
    assert not A.handoff_stuck(events, "ubuntu").passed

def test_none_loader_entry():
    line = "[ INFO]: myboot:   'UEFI QEMU HARDDISK' loader=None health=Healthy"
    e = logparse.parse_line(line)
    assert isinstance(e, ev.Entry) and e.loader is None


def test_timing_line():
    line = "[ INFO]: crates/engine/src/timing.rs@028: myboot: timing discovery 1234us total=5678us"
    e = logparse.parse_line(line)
    assert e == ev.Timing(stage="discovery", us=1234, total_us=5678)
