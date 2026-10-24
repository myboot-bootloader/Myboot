//! Boot-method refinement (report §3.3). Discovery's BLS source assumes a Linux
//! kernel is an EFI stub (the common case). Here we confirm it by peeking the
//! kernel file: a valid PE/COFF EFI application → `LinuxEfiStub` (firmware loads
//! it, Secure-Boot friendly); anything else (a raw ELF/bzImage without the EFI
//! stub) → `LinuxDirect`, which routes to the `arch` boot-protocol handoff.
//!
//! This is what makes the direct-boot path reachable ONLY when it is genuinely
//! needed, and keeps the EFI-stub chainload as the default per best practice.
extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use ports::FileStore;
use graph::BootMethod;
use storage::pe::PeHeader;
use crate::RawEntry;

/// First read: enough for every real kernel's DOS + PE headers (`e_lfanew` is
/// typically 0x40..0x200).
const PE_PROBE: usize = 4 * 1024;
/// Ceiling for a second read when `e_lfanew` points past the first probe. Bounds
/// the work a hostile or odd file can cause.
const PE_PROBE_MAX: usize = 64 * 1024;

/// Adjust the boot method of each Linux kernel entry by inspecting the kernel
/// file. Only entries that already carry a kernel path are considered. Only the
/// PE header is read (never the whole kernel), and each distinct kernel path is
/// classified once — many NixOS generations share one kernel.
pub fn refine_methods<F: FileStore + ?Sized>(mut raw: Vec<RawEntry>, fs: &F) -> Vec<RawEntry> {
    let mut verdicts: BTreeMap<String, Option<bool>> = BTreeMap::new();
    for e in raw.iter_mut() {
        if e.method != BootMethod::LinuxEfiStub { continue; }
        let kernel = match &e.kernel { Some(k) => k.clone(), None => continue };
        let verdict = *verdicts.entry(kernel).or_insert_with_key(|k| is_efi_stub(fs, k));
        // None = unreadable/unclassifiable → keep the EFI-stub assumption.
        if verdict == Some(false) { e.method = BootMethod::LinuxDirect; } // not an EFI stub → direct
    }
    raw
}

/// Some(true) = PE EFI application, Some(false) = not one, None = could not be
/// read, or its PE header lies beyond `PE_PROBE_MAX` (assumption kept).
fn is_efi_stub<F: FileStore + ?Sized>(fs: &F, kernel: &str) -> Option<bool> {
    let mut head = fs.read_head(kernel, PE_PROBE).ok()?;
    if let Some(need) = pe_bytes_needed(&head) {
        if need > head.len() && head.len() == PE_PROBE {
            if need > PE_PROBE_MAX { return None; }
            head = fs.read_head(kernel, need).ok()?;
        }
    }
    Some(matches!(PeHeader::parse(&head), Ok(pe) if pe.is_efi_application))
}

/// Bytes `PeHeader::parse` touches: e_lfanew + "PE\0\0" (4) + COFF (20) +
/// subsystem offset (68) + u16 (2). Mirrors `storage/src/pe/mod.rs`.
fn pe_bytes_needed(head: &[u8]) -> Option<usize> {
    let lfanew = u32::from_le_bytes(head.get(0x3C..0x40)?.try_into().ok()?) as usize;
    lfanew.checked_add(4 + 20 + 68 + 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ports::mock::MockFileStore;
    use graph::{OsKind, EntryRole};

    fn linux_entry(kernel: &str) -> RawEntry {
        let mut e = RawEntry::new(OsKind::Linux, "Linux", "Linux", EntryRole::Default, BootMethod::LinuxEfiStub);
        e.kernel = Some(kernel.into());
        e
    }

    // A minimal valid PE/COFF EFI-application header (MZ + PE + PE32+ subsystem 10).
    fn efi_stub_bytes() -> alloc::vec::Vec<u8> {
        let mut b = alloc::vec![0u8; 0x100];
        b[0] = b'M'; b[1] = b'Z';
        let pe_off = 0x80u32;
        b[0x3C..0x40].copy_from_slice(&pe_off.to_le_bytes());
        let po = pe_off as usize;
        b[po..po+4].copy_from_slice(b"PE\0\0");
        // optional header magic PE32+ (0x20b) at po+24
        b[po+24..po+26].copy_from_slice(&0x20bu16.to_le_bytes());
        // subsystem (u16) at po+24+68 = po+92 for PE32+
        b[po+92..po+94].copy_from_slice(&10u16.to_le_bytes());
        b
    }

    #[test]
    fn efi_stub_kernel_stays_efi_stub() {
        let fs = MockFileStore::new().with_file("\\k\\bzImage", &efi_stub_bytes());
        let out = refine_methods(alloc::vec![linux_entry("\\k\\bzImage")], &fs);
        assert_eq!(out[0].method, BootMethod::LinuxEfiStub);
    }

    #[test]
    fn non_pe_kernel_becomes_direct() {
        // A raw ELF-ish kernel (no MZ) → direct boot via the arch handoff.
        let fs = MockFileStore::new().with_file("\\k\\vmlinux", b"\x7fELF and the rest");
        let out = refine_methods(alloc::vec![linux_entry("\\k\\vmlinux")], &fs);
        assert_eq!(out[0].method, BootMethod::LinuxDirect);
    }

    #[test]
    fn unreadable_kernel_keeps_the_assumption() {
        let fs = MockFileStore::new(); // file absent
        let out = refine_methods(alloc::vec![linux_entry("\\k\\missing")], &fs);
        assert_eq!(out[0].method, BootMethod::LinuxEfiStub);
    }

    #[test]
    fn pe_header_beyond_first_probe_is_still_classified() {
        // e_lfanew past the 4 KiB first read → a second, bounded read.
        let mut b = alloc::vec![0u8; 0x2000];
        b[0] = b'M'; b[1] = b'Z';
        let po = 0x1100usize;
        b[0x3C..0x40].copy_from_slice(&(po as u32).to_le_bytes());
        b[po..po+4].copy_from_slice(b"PE\0\0");
        b[po+24..po+26].copy_from_slice(&0x20bu16.to_le_bytes());
        b[po+92..po+94].copy_from_slice(&10u16.to_le_bytes());
        let fs = MockFileStore::new().with_file("\\k\\far", &b);
        let out = refine_methods(alloc::vec![linux_entry("\\k\\far")], &fs);
        assert_eq!(out[0].method, BootMethod::LinuxEfiStub);
    }

    #[test]
    fn pe_header_beyond_ceiling_keeps_the_assumption() {
        // e_lfanew past PE_PROBE_MAX: not read further, not downgraded to direct.
        let mut b = alloc::vec![0u8; PE_PROBE];
        b[0] = b'M'; b[1] = b'Z';
        b[0x3C..0x40].copy_from_slice(&(0x20_000u32).to_le_bytes());
        let fs = MockFileStore::new().with_file("\\k\\odd", &b);
        let out = refine_methods(alloc::vec![linux_entry("\\k\\odd")], &fs);
        assert_eq!(out[0].method, BootMethod::LinuxEfiStub);
    }

    #[test]
    fn shared_kernel_is_read_once_and_only_its_header() {
        // Counts calls so a regression to whole-file or per-entry reads fails.
        struct Counting { inner: MockFileStore, heads: core::cell::Cell<usize>, reads: core::cell::Cell<usize> }
        impl FileStore for Counting {
            fn read(&self, p: &str) -> ports::IoResult<Vec<u8>> { self.reads.set(self.reads.get() + 1); self.inner.read(p) }
            fn read_head(&self, p: &str, n: usize) -> ports::IoResult<Vec<u8>> { self.heads.set(self.heads.get() + 1); self.inner.read_head(p, n) }
            fn write(&mut self, p: &str, d: &[u8]) -> ports::IoResult<()> { self.inner.write(p, d) }
            fn append(&mut self, p: &str, d: &[u8]) -> ports::IoResult<()> { self.inner.append(p, d) }
            fn exists(&self, p: &str) -> bool { self.inner.exists(p) }
            fn list_dir(&self, p: &str) -> ports::IoResult<Vec<String>> { self.inner.list_dir(p) }
            fn list_dir_meta(&self, p: &str) -> ports::IoResult<Vec<ports::DirEntryMeta>> { self.inner.list_dir_meta(p) }
        }
        let fs = Counting {
            inner: MockFileStore::new().with_file("\\k\\bzImage", &efi_stub_bytes()),
            heads: core::cell::Cell::new(0), reads: core::cell::Cell::new(0),
        };
        let raw = alloc::vec![linux_entry("\\k\\bzImage"), linux_entry("\\k\\bzImage"), linux_entry("\\k\\bzImage")];
        let out = refine_methods(raw, &fs);
        assert!(out.iter().all(|e| e.method == BootMethod::LinuxEfiStub));
        assert_eq!(fs.heads.get(), 1, "one header read for a kernel shared by three entries");
        assert_eq!(fs.reads.get(), 0, "never a whole-file read");
    }
}
