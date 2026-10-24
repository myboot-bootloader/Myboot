//! Stable identifiers for the Boot Graph (report §3.5). `EntryId` is the SSOT
//! key that config, NVRAM, logs and the transaction manager all refer to.
extern crate alloc;
use crate::kinds::{EntryRole, OsKind};
use alloc::string::String;
use core::fmt;

/// A stable identifier for a boot entry. STABLE ACROSS REBOOTS: derived
/// deterministically from durable attributes (OS kind, role, loader path), never
/// from discovery order, so references stay valid over time (report §3.5).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(String);

impl EntryId {
    /// Build a stable id from durable parts. Deterministic: same inputs → same id.
    pub fn derive(kind: OsKind, role: &EntryRole, loader: Option<&str>) -> Self {
        let mut s = String::new();
        s.push_str(kind.slug());
        s.push(':');
        role.write_slug(&mut s);
        if let Some(l) = loader {
            s.push(':');
            // normalise path separators so \\ and / compare equal
            for c in l.chars() {
                s.push(if c == '\\' {
                    '/'
                } else {
                    c.to_ascii_lowercase()
                });
            }
        }
        EntryId(s)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Like [`derive`](Self::derive), but scoped to a specific volume so the SAME
    /// loader path on two different volumes (e.g. `\EFI\BOOT\BOOTX64.EFI` on both
    /// MyBoot's own medium and a Ventoy USB) yields DISTINCT ids. Multi-disk
    /// disambiguation: `<base>@<volume>`. A `None` volume reproduces `derive`
    /// exactly, so single-volume ids and configs are unchanged.
    pub fn derive_scoped(
        kind: OsKind,
        role: &EntryRole,
        loader: Option<&str>,
        volume: Option<&str>,
    ) -> Self {
        let mut id = Self::derive(kind, role, loader);
        if let Some(v) = volume {
            id.0.push('@');
            for c in v.chars() {
                id.0.push(if c == '\\' {
                    '/'
                } else {
                    c.to_ascii_lowercase()
                });
            }
        }
        id
    }
    /// Construct from a raw string (e.g. read back from config/NVRAM).
    pub fn from_raw(s: impl Into<String>) -> Self {
        EntryId(s.into())
    }
}
impl fmt::Debug for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EntryId({})", self.0)
    }
}

/// Identifier for a physical disk in the graph.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DiskId(pub u64);
