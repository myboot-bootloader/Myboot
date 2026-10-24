//! MyBoot — UEFI application entry point (report §4.4). Compiles to BOOTX64.EFI.
//!
//! Deliberately tiny: initialise uefi-rs, then hand off to the engine. All logic
//! lives in `engine` and the crates it composes. On success the engine hands off
//! to an OS and never returns; on failure we log and return a firmware error so
//! the firmware can try the next boot option.
#![no_main]
#![no_std]

use uefi::prelude::*;

#[entry]
fn efi_main() -> Status {
    // Installs the global allocator, logger, and panic handler.
    if uefi::helpers::init().is_err() {
        return Status::ABORTED;
    }
    // Show info-level diagnostics on the serial console (the logger defaults can
    // hide anything below error; boot observability wants the pipeline summary).
    log::set_max_level(log::LevelFilter::Info);
    match engine::run() {
        Ok(never) => match never {}, // Infallible: handoff never returns
        Err(e) => {
            log::error!("myboot: engine could not hand off: {e:?}");
            Status::LOAD_ERROR
        }
    }
}
