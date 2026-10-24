//! storage — parsers over untrusted on-disk bytes (report §3.12, §4.6, §5.4).
//!
//! HARD RULE (blueprint §1.2): every parser is a TOTAL function over `&[u8]`.
//! It returns `Ok(..)` or `Err(..)` and NEVER panics, indexes a slice directly,
//! overflows, or reads out of bounds. All access goes through the checked
//! helpers in `bytes`. These are the exact functions the Kani harnesses verify.
//!
//! Pure and firmware-independent (no uefi) so it is unit-tested on the host and
//! model-checked by Kani. The `platform::block` adapter feeds it raw sectors.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

pub mod bytes;
pub mod fat;
pub mod gpt;
pub mod mbr;
pub mod pe;

/// A parsed partition, format-agnostic, handed up to discovery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Partition {
    pub start_lba: u64,
    pub end_lba: u64,
    pub kind: PartKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    EfiSystem,
    Basic,
    Linux,
    Unknown,
}

#[cfg(kani)]
mod proofs;

#[cfg(test)]
mod tests;
