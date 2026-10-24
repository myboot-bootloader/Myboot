//! platform — the UEFI adapter (report §3.4, §4.4–4.6; ADR 0003 outer ring).
//!
//! This is the ONLY crate (besides `arch`) that knows uefi-rs exists. It
//! IMPLEMENTS the driven ports the domain core defined (`VarStore`, `FileStore`)
//! against real firmware services, and provides graphics (GOP) and Secure Boot
//! reads. Nothing above this crate calls uefi-rs directly — swap this crate and
//! the core is unchanged.
//!
//! Written against uefi-rs 0.33 (the current global model: `uefi::boot` /
//! `uefi::runtime` free functions, no `SystemTable<Boot>` parameter). It requires
//! the `x86_64-unknown-uefi` target to compile; see BUILD_AND_TROUBLESHOOT.md.
#![no_std]
#![no_main]
extern crate alloc;

pub mod fs;
pub mod gop;
pub mod secureboot;
pub mod vars;
pub mod volume;

pub use fs::UefiFileStore;
pub use gop::Framebuffer;
pub use vars::UefiVarStore;
pub use volume::{own_volume_id, volumes, MultiVolume, Volume};
