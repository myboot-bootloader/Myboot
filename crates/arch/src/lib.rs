#![no_std]

//! arch — architecture-specific low-level support and the project's ONLY
//! hand-written assembly (report §1.7, §4.4).
//!
//! IMPORTANT — what assembly is and is NOT for here:
//!   * Chainloading (Windows Boot Manager, Linux EFI-stub, generic .efi) uses
//!     the firmware's LoadImage/StartImage and needs NO assembly.
//!   * Assembly is required ONLY for:
//!     1. the final handoff when DIRECTLY loading a kernel (LinuxDirect):
//!        set registers/stack per the boot protocol and jump to the entry
//!        point AFTER ExitBootServices (see x86_64/handoff.s);
//!     2. a few privileged CPU operations not exposed safely by core
//!        (cli/hlt, control-register access) — see x86_64/cpu.rs.
//!   * Everything else in MyBoot is Rust. Keep this crate deliberately tiny.

extern crate alloc;

#[cfg(test)]
extern crate std;

#[cfg(target_arch = "x86_64")]
pub mod x86_64;
