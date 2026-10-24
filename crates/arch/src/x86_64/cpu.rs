//! Privileged CPU operations via inline assembly (report §4.4). Each wrapper is
//! the minimal real primitive the boot manager needs; there is no speculative
//! surface here. Used by the recovery/halt path and by the handoff sequence.
use core::arch::asm;

/// Disable maskable interrupts (`cli`). Called before the final handoff and on
/// the unrecoverable path.
#[inline]
pub fn disable_interrupts() {
    unsafe {
        asm!("cli", options(nomem, nostack, preserves_flags));
    }
}

/// Halt the CPU forever (interrupts already disabled). Used as the terminal
/// state when there is nothing left to do — never returns.
#[inline]
pub fn halt_forever() -> ! {
    unsafe {
        asm!("cli", options(nomem, nostack, preserves_flags));
        loop {
            asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

/// Read CR3 (the page-table base) — used to hand the current paging root to the
/// kernel handoff, since the Linux 64-bit entry keeps paging enabled.
///
/// # Safety
/// Requires CPL 0 (always true in a UEFI boot application).
#[inline]
pub unsafe fn read_cr3() -> u64 {
    let value: u64;
    asm!("mov {}, cr3", out(reg) value, options(nomem, nostack, preserves_flags));
    value
}
