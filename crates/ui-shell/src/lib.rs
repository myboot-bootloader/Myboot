//! ui-shell — the pre-boot diagnostic shell (report §3.10.2, Figure 3.7). A
//! driving adapter (inbound port) over the Boot Graph: it turns typed command
//! lines into read-only queries and boot requests. It contains NO boot logic and
//! NO firmware I/O — it produces `ShellOutput` (text lines) and `ShellAction`
//! (things the engine should do), so the parser and renderer are host-tested.
//!
//! Mirrors the ergonomics of the systemd-boot menu and rEFInd's shell, but scoped
//! to management: list/inspect/health/verify/boot/default/log/reboot.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

pub mod commands;

#[cfg(test)]
extern crate std;

pub use commands::{parse, render, Command, ShellAction};
