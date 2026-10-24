//! security — Secure Boot posture and image-launch policy (report §3.13).
//!
//! MyBoot's security stance mirrors the modern, pragmatic approach (Lanzaboote,
//! systemd-boot): PREFER to let the firmware verify images via `LoadImage`, so we
//! inherit the platform's trust chain instead of re-implementing crypto in the
//! boot path. We only make an EXPLICIT signature decision when we load an image
//! ourselves (the direct-kernel path), and even then the verified-crypto routine
//! is the one place `spec/proofs/fstar` may later be linked in.
//!
//! This crate is pure policy over a reported Secure Boot state (queried from
//! firmware by the `platform` adapter), so it is host-tested without firmware.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

pub mod policy;
pub mod state;

#[cfg(test)]
extern crate std;

pub use policy::{decide_launch, LaunchDecision, Verification};
pub use state::SecureBootState;
