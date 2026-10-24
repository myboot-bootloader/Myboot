//! The direct-kernel boot path (report §3.6 LinuxDirect, §4.4). This is the
//! firmware side that the pure `arch` boot-protocol structures and trampoline
//! were built for: it loads a raw bzImage, builds its `boot_params` (zero page)
//! from the image's `setup_header` + the command line + the initrd + an E820 map
//! derived from the UEFI memory map, exits boot services, and jumps to the
//! kernel via `arch::x86_64::handoff`.
//!
//! Design note (evidence-based): the DEFAULT Linux path in MyBoot is the EFI-stub
//! chainload (see `pipeline::launch_chainload`), because the kernel maintainers
//! deprecated the EFI handover protocol and the stub path is Secure-Boot-verified
//! (LWN "The bootstrap process on EFI systems"; kernel EFI list). This direct
//! path exists for kernels WITHOUT a usable EFI stub and is selected only when a
//! provider yields `LaunchPlan::DirectKernel`. It follows the authoritative
//! 64-bit boot protocol in Documentation/arch/x86/boot.rst.
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use uefi::boot::{self, AllocateType, MemoryType};
use uefi::mem::memory_map::MemoryMap;

use arch::x86_64::boot_params::{
    e820, BootParams, E820Entry, SetupHeader, ENTRY_OFFSET_64, SETUP_HEADER_OFFSET,
};
use platform::UefiFileStore;
use ports::FileStore;

use crate::pipeline::EngineError;

/// Perform a direct kernel boot. Reads the kernel and initrd from the ESP, builds
/// the zero page, exits boot services, and hands off. Never returns on success.
pub fn launch(
    kernel_path: &str,
    initrd_path: Option<&str>,
    cmdline: Option<&str>,
) -> Result<core::convert::Infallible, EngineError> {
    let fs = UefiFileStore::new();

    // 1. Load the kernel image and validate its setup_header.
    let kernel = fs
        .read(kernel_path)
        .map_err(|_| EngineError::Firmware(uefi::Status::NOT_FOUND))?;
    let hdr = read_setup_header(&kernel).ok_or(EngineError::Aborted)?;
    if !hdr.is_valid() || !hdr.has_64bit_entry() {
        return Err(EngineError::Aborted); // not a bootable 64-bit bzImage
    }

    // 2. Place the kernel's protected-mode body at a page-aligned physical load
    //    address and compute the 64-bit entry point (load + 0x200).
    let setup_sects = if hdr.setup_sects == 0 {
        4
    } else {
        hdr.setup_sects
    } as usize;
    let real_mode_size = (setup_sects + 1) * 512; // setup code (skipped at 64-bit entry)
    let protected = &kernel[real_mode_size..];
    let kernel_phys = alloc_pages_for(protected.len())?;
    unsafe {
        core::ptr::copy_nonoverlapping(protected.as_ptr(), kernel_phys as *mut u8, protected.len());
    }
    let entry = kernel_phys + ENTRY_OFFSET_64;

    // 3. Load the initrd (if any) into memory and record its address/size.
    let (rd_addr, rd_size) = match initrd_path {
        Some(p) => {
            let rd = fs
                .read(p)
                .map_err(|_| EngineError::Firmware(uefi::Status::NOT_FOUND))?;
            let addr = alloc_pages_for(rd.len())?;
            unsafe {
                core::ptr::copy_nonoverlapping(rd.as_ptr(), addr as *mut u8, rd.len());
            }
            (addr as u32, rd.len() as u32)
        }
        None => (0, 0),
    };

    // 4. Copy the command line into a page and NUL-terminate it.
    let cmd = cmdline.unwrap_or("");
    let cmd_addr = alloc_pages_for(cmd.len() + 1)?;
    unsafe {
        core::ptr::copy_nonoverlapping(cmd.as_ptr(), cmd_addr as *mut u8, cmd.len());
        *((cmd_addr + cmd.len() as u64) as *mut u8) = 0;
    }

    // 5. Build the zero page: header + loader fields. E820 is filled AFTER we
    //    exit boot services (only then is the map final).
    let mut bp = BootParams::new();
    bp.set_setup_header(&hdr);
    bp.set_loader_fields(cmd_addr as u32, rd_addr, rd_size);

    // 6. Allocate a kernel stack.
    let stack = alloc_pages_for(64 * 1024)?;
    let stack_top = stack + 64 * 1024;

    // 7. Exit boot services, convert the returned UEFI map to E820, finalise the
    //    zero page, then jump. After exit we make NO more firmware calls.
    // uefi 0.33: exit_boot_services takes a plain `MemoryType` (the wrapping in
    // `Option<MemoryType>` was introduced later, in 0.35).
    let mmap = unsafe { boot::exit_boot_services(MemoryType::LOADER_DATA) };
    let e820 = to_e820(&mmap);
    bp.set_e820(&e820);

    arch::x86_64::cpu::disable_interrupts();
    unsafe {
        arch::x86_64::handoff::jump_to_kernel(entry, bp.as_ptr() as u64, stack_top);
    }
}

/// Read the setup_header out of a kernel image at offset 0x1f1.
fn read_setup_header(kernel: &[u8]) -> Option<SetupHeader> {
    let size = core::mem::size_of::<SetupHeader>();
    if kernel.len() < SETUP_HEADER_OFFSET + size {
        return None;
    }
    let mut hdr = SetupHeader::default();
    let dst = unsafe { core::slice::from_raw_parts_mut(&mut hdr as *mut _ as *mut u8, size) };
    dst.copy_from_slice(&kernel[SETUP_HEADER_OFFSET..SETUP_HEADER_OFFSET + size]);
    Some(hdr)
}

/// Allocate whole pages covering `bytes` and return the physical base address.
fn alloc_pages_for(bytes: usize) -> Result<u64, EngineError> {
    let pages = bytes.div_ceil(0x1000);
    boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        pages.max(1),
    )
    .map(|p| p.as_ptr() as u64)
    .map_err(|e| EngineError::Firmware(e.status()))
}

/// Convert a UEFI memory map into an E820 table, coalescing adjacent regions of
/// the same E820 type (the kernel expects a compact map).
fn to_e820(mmap: &uefi::mem::memory_map::MemoryMapOwned) -> Vec<E820Entry> {
    let mut out: Vec<E820Entry> = Vec::new();
    for desc in mmap.entries() {
        let kind = e820_type(desc.ty);
        let addr = desc.phys_start;
        let size = desc.page_count * 0x1000;
        if size == 0 {
            continue;
        }
        // coalesce with the previous entry if contiguous and same type
        if let Some(last) = out.last_mut() {
            let last_end = last.addr.wrapping_add(last.size);
            if last.kind == kind && last_end == addr {
                last.size += size;
                continue;
            }
        }
        out.push(E820Entry { addr, size, kind });
    }
    out
}

/// Map UEFI memory types to E820 types (the standard loader mapping).
fn e820_type(ty: MemoryType) -> u32 {
    match ty {
        MemoryType::CONVENTIONAL
        | MemoryType::BOOT_SERVICES_CODE
        | MemoryType::BOOT_SERVICES_DATA
        | MemoryType::LOADER_CODE
        | MemoryType::LOADER_DATA => e820::RAM,
        MemoryType::ACPI_RECLAIM => e820::ACPI,
        MemoryType::ACPI_NON_VOLATILE => e820::NVS,
        MemoryType::UNUSABLE => e820::UNUSABLE,
        // runtime services, MMIO, reserved, etc. → reserved for the OS
        _ => e820::RESERVED,
    }
}

// Silence unused-import lints for String on builds where cmdline handling changes.
const _: fn(String) = |_| {};
