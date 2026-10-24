//! persistence — MyBoot's three-tier state, with NO database (report §3.9; ADR 0001).
//!
//! Tier 1 — firmware variables: tiny must-persist state, via the `VarStore` port.
//! Tier 2 — ESP files: the declarative config + the growing boot log, via `FileStore`.
//! Tier 3 — the in-memory Boot Graph: derived, never persisted (lives in `graph`).
//!
//! This crate owns the *encoding* of persisted data (pure, panic-free, host-tested)
//! and the thin store objects that move it through the ports. It knows nothing of
//! uefi — the actual firmware calls live in the `platform` adapter.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

pub mod loadopt;
pub mod log;
pub mod nvram;

#[cfg(test)]
extern crate std;
