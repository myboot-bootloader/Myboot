//! The Linux x86 boot-protocol data structures (report §3.6, §4.4). These are
//! the exact on-the-wire layouts the kernel reads at handoff — the "zero page"
//! (`boot_params`), its embedded `setup_header`, and the E820 memory map — per
//! Documentation/arch/x86/boot.rst and arch/x86/include/uapi/asm/bootparam.h.
//!
//! This module is firmware-agnostic and fully host-tested: it only defines the
//! layout and the safe helpers to populate it. The engine fills these from the
//! kernel image + the UEFI memory map; the assembly trampoline consumes the
//! finished `boot_params` (its base in %rsi).
extern crate alloc;

/// Offset of the 64-bit kernel entry point from the loaded kernel base.
/// (kernel.org: "jumping to the 64-bit kernel entry point, which is the start
/// address of loaded 64-bit kernel plus 0x200".)
pub const ENTRY_OFFSET_64: u64 = 0x200;

/// Offset of `setup_header` within the kernel image / boot_params (0x1f1).
pub const SETUP_HEADER_OFFSET: usize = 0x1f1;

/// The magic `header` field value ("HdrS") at offset 0x202 of a valid image.
pub const HDRS_MAGIC: u32 = 0x5372_6448; // little-endian "HdrS"

/// `type_of_loader` value for an undefined/other loader with our own version.
/// 0xFF = "undefined" per the protocol (acceptable when no ID is assigned).
pub const LOADER_TYPE_UNDEFINED: u8 = 0xFF;

/// LOADED_HIGH: the protected-mode kernel is loaded high (bit 0 of loadflags).
pub const LOADFLAG_LOADED_HIGH: u8 = 0x01;
/// CAN_USE_HEAP: heap_end_ptr is valid (bit 7 of loadflags).
pub const LOADFLAG_CAN_USE_HEAP: u8 = 0x80;

/// One E820 memory-map entry (packed, as the kernel expects).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct E820Entry {
    pub addr: u64,
    pub size: u64,
    pub kind: u32,
}

/// E820 memory types (the ones a loader emits).
pub mod e820 {
    pub const RAM: u32 = 1;
    pub const RESERVED: u32 = 2;
    pub const ACPI: u32 = 3; // ACPI reclaimable
    pub const NVS: u32 = 4; // ACPI NVS
    pub const UNUSABLE: u32 = 5;
}

/// Maximum E820 entries the zero page can hold (kernel `E820_MAX_ENTRIES_ZEROPAGE`).
pub const E820_MAX_ENTRIES: usize = 128;

/// The setup_header, at offset 0x1f1 of the image. Only the fields a loader must
/// read or write are named; the layout matches the kernel header exactly, so it
/// can be copied byte-for-byte out of the kernel image.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SetupHeader {
    pub setup_sects: u8,
    pub root_flags: u16,
    pub syssize: u32,
    pub ram_size: u16,
    pub vid_mode: u16,
    pub root_dev: u16,
    pub boot_flag: u16,
    pub jump: u16,
    pub header: u32, // "HdrS"
    pub version: u16,
    pub realmode_swtch: u32,
    pub start_sys_seg: u16,
    pub kernel_version: u16,
    pub type_of_loader: u8,
    pub loadflags: u8,
    pub setup_move_size: u16,
    pub code32_start: u32,
    pub ramdisk_image: u32, // initrd physical address
    pub ramdisk_size: u32,  // initrd size in bytes
    pub bootsect_kludge: u32,
    pub heap_end_ptr: u16,
    pub ext_loader_ver: u8,
    pub ext_loader_type: u8,
    pub cmd_line_ptr: u32, // physical address of the NUL-terminated cmdline
    pub initrd_addr_max: u32,
    pub kernel_alignment: u32,
    pub relocatable_kernel: u8,
    pub min_alignment: u8,
    pub xloadflags: u16,
    pub cmdline_size: u32,
    pub hardware_subarch: u32,
    pub hardware_subarch_data: u64,
    pub payload_offset: u32,
    pub payload_length: u32,
    pub setup_data: u64,
    pub pref_address: u64,
    pub init_size: u32,
    pub handover_offset: u32,
    pub kernel_info_offset: u32,
}

impl SetupHeader {
    /// True if this looks like a valid, bootable x86 kernel image header.
    pub fn is_valid(&self) -> bool {
        let magic = self.header; // avoid unaligned reference to packed field
        let flag = self.boot_flag;
        magic == HDRS_MAGIC && flag == 0xAA55
    }

    /// XLF_KERNEL_64: the image has the legacy 64-bit entry at 0x200.
    pub fn has_64bit_entry(&self) -> bool {
        let xlf = self.xloadflags;
        xlf & 0x1 != 0
    }
}

/// The zero page. We model it as a fixed 4 KiB block and place the fields the
/// loader writes at their exact offsets, which is safer than naming all ~100
/// legacy fields. Accessors use explicit offsets so the layout is unmistakable.
#[repr(C, align(4096))]
pub struct BootParams {
    bytes: [u8; 4096],
}

impl Default for BootParams {
    fn default() -> Self {
        BootParams { bytes: [0u8; 4096] }
    }
}

impl BootParams {
    // Field offsets within the zero page (arch/x86/include/uapi/asm/bootparam.h).
    const E820_ENTRIES_OFF: usize = 0x1e8; // u8 count
    const E820_TABLE_OFF: usize = 0x2d0; // E820Entry[E820_MAX_ENTRIES]
    const SETUP_HEADER_OFF: usize = 0x1f1; // setup_header

    pub fn new() -> Self {
        Self::default()
    }

    /// Raw base pointer of the zero page (what goes into %rsi at handoff).
    pub fn as_ptr(&self) -> *const u8 {
        self.bytes.as_ptr()
    }

    /// Copy a validated setup_header into the zero page at offset 0x1f1.
    pub fn set_setup_header(&mut self, hdr: &SetupHeader) {
        let src = unsafe {
            core::slice::from_raw_parts(
                hdr as *const _ as *const u8,
                core::mem::size_of::<SetupHeader>(),
            )
        };
        let dst = &mut self.bytes[Self::SETUP_HEADER_OFF..Self::SETUP_HEADER_OFF + src.len()];
        dst.copy_from_slice(src);
    }

    /// Read the setup_header back out (used to patch loader fields).
    pub fn setup_header(&self) -> SetupHeader {
        let mut hdr = SetupHeader::default();
        let dst = unsafe {
            core::slice::from_raw_parts_mut(
                &mut hdr as *mut _ as *mut u8,
                core::mem::size_of::<SetupHeader>(),
            )
        };
        let src = &self.bytes[Self::SETUP_HEADER_OFF..Self::SETUP_HEADER_OFF + dst.len()];
        dst.copy_from_slice(src);
        hdr
    }

    /// Patch the loader-owned setup_header fields: type, cmdline, ramdisk.
    pub fn set_loader_fields(&mut self, cmd_line_ptr: u32, ramdisk_image: u32, ramdisk_size: u32) {
        let mut hdr = self.setup_header();
        hdr.type_of_loader = LOADER_TYPE_UNDEFINED;
        hdr.loadflags |= LOADFLAG_LOADED_HIGH | LOADFLAG_CAN_USE_HEAP;
        hdr.cmd_line_ptr = cmd_line_ptr;
        hdr.ramdisk_image = ramdisk_image;
        hdr.ramdisk_size = ramdisk_size;
        self.set_setup_header(&hdr);
    }

    /// Write the E820 table and its entry count. Truncates to E820_MAX_ENTRIES.
    pub fn set_e820(&mut self, entries: &[E820Entry]) {
        let n = entries.len().min(E820_MAX_ENTRIES);
        self.bytes[Self::E820_ENTRIES_OFF] = n as u8;
        let entry_size = core::mem::size_of::<E820Entry>();
        for (i, e) in entries.iter().take(n).enumerate() {
            let off = Self::E820_TABLE_OFF + i * entry_size;
            let src =
                unsafe { core::slice::from_raw_parts(e as *const _ as *const u8, entry_size) };
            self.bytes[off..off + entry_size].copy_from_slice(src);
        }
    }

    /// Number of E820 entries currently recorded.
    pub fn e820_count(&self) -> u8 {
        self.bytes[Self::E820_ENTRIES_OFF]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_header_validates_magic_and_boot_flag() {
        let mut h = SetupHeader::default();
        assert!(!h.is_valid());
        h.header = HDRS_MAGIC;
        h.boot_flag = 0xAA55;
        assert!(h.is_valid());
    }

    #[test]
    fn xloadflags_reports_64bit_entry() {
        let mut h = SetupHeader::default();
        h.xloadflags = 0x1;
        assert!(h.has_64bit_entry());
        h.xloadflags = 0x0;
        assert!(!h.has_64bit_entry());
    }

    #[test]
    fn boot_params_roundtrips_setup_header_at_0x1f1() {
        let mut bp = BootParams::new();
        let mut h = SetupHeader::default();
        h.header = HDRS_MAGIC;
        h.boot_flag = 0xAA55;
        h.cmd_line_ptr = 0x9_0000;
        bp.set_setup_header(&h);
        let back = bp.setup_header();
        assert!(back.is_valid());
        let ptr = back.cmd_line_ptr;
        assert_eq!(ptr, 0x9_0000);
    }

    #[test]
    fn loader_fields_are_patched_in_place() {
        let mut bp = BootParams::new();
        let mut h = SetupHeader::default();
        h.header = HDRS_MAGIC;
        h.boot_flag = 0xAA55;
        bp.set_setup_header(&h);
        bp.set_loader_fields(0x20000, 0x1000000, 0x40000);
        let back = bp.setup_header();
        let (cl, ri, rs, lt) = (
            back.cmd_line_ptr,
            back.ramdisk_image,
            back.ramdisk_size,
            back.type_of_loader,
        );
        assert_eq!(cl, 0x20000);
        assert_eq!(ri, 0x1000000);
        assert_eq!(rs, 0x40000);
        assert_eq!(lt, LOADER_TYPE_UNDEFINED);
        assert!(back.loadflags & LOADFLAG_LOADED_HIGH != 0);
    }

    #[test]
    fn e820_table_and_count_are_written() {
        let mut bp = BootParams::new();
        let entries = alloc::vec![
            E820Entry {
                addr: 0,
                size: 0x9_fc00,
                kind: e820::RAM
            },
            E820Entry {
                addr: 0x10_0000,
                size: 0x7000_0000,
                kind: e820::RAM
            },
            E820Entry {
                addr: 0xE000_0000,
                size: 0x1000_0000,
                kind: e820::RESERVED
            },
        ];
        bp.set_e820(&entries);
        assert_eq!(bp.e820_count(), 3);
        // read the second entry straight back out of the zero page
        let entry_size = core::mem::size_of::<E820Entry>();
        let off = 0x2d0 + entry_size;
        let raw = &bp.bytes[off..off + entry_size];
        let mut got = E820Entry::default();
        unsafe {
            core::slice::from_raw_parts_mut(&mut got as *mut _ as *mut u8, entry_size)
                .copy_from_slice(raw);
        }
        assert_eq!(
            got,
            E820Entry {
                addr: 0x10_0000,
                size: 0x7000_0000,
                kind: e820::RAM
            }
        );
    }

    #[test]
    fn e820_truncates_to_max() {
        let mut bp = BootParams::new();
        let many =
            alloc::vec![E820Entry { addr: 0, size: 1, kind: e820::RAM }; E820_MAX_ENTRIES + 10];
        bp.set_e820(&many);
        assert_eq!(bp.e820_count() as usize, E820_MAX_ENTRIES);
    }
}
