//! Health annotations and boot outcomes (report §3.5, §3.7). Health is always a
//! machine-readable reason, never a bare present/absent flag — policy, the UIs
//! and recovery all consume it.
extern crate alloc;
use alloc::string::String;

/// A human-readable explanation attached to a health state or decision.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reason(pub String);
impl Reason {
    pub fn new(s: impl Into<String>) -> Self {
        Reason(s.into())
    }
}

/// First-class health annotation consumed by policy, the UIs and recovery.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Health {
    Healthy,
    Degraded(Reason),
    Unbootable(Reason),
    Unknown,
}
impl Health {
    /// Can this entry be booted at all? Unbootable/Unknown are not candidates.
    pub fn is_bootable(&self) -> bool {
        matches!(self, Health::Healthy | Health::Degraded(_))
    }
    /// Rank for default-selection: Healthy > Degraded > Unknown > Unbootable.
    pub fn rank(&self) -> u8 {
        match self {
            Health::Healthy => 3,
            Health::Degraded(_) => 2,
            Health::Unknown => 1,
            Health::Unbootable(_) => 0,
        }
    }
}

/// The recorded outcome of the last attempt on an entry (report §3.7).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootResult {
    Confirmed,
    Failed,
    Unknown,
}
