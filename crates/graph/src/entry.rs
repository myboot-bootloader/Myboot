//! A single bootable target — the leaf of the Boot Graph (report §3.5).
extern crate alloc;
use crate::health::{BootResult, Health};
use crate::ids::EntryId;
use crate::kinds::{BootMethod, EntryRole, OsKind};
use alloc::string::String;

/// Filesystem path to an EFI file, e.g. `\\EFI\\Microsoft\\Boot\\bootmgfw.efi`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EfiPath(pub String);

/// A single bootable target.
#[derive(Clone, Debug)]
pub struct BootEntry {
    pub id: EntryId,
    pub title: String,
    pub role: EntryRole,
    pub method: BootMethod,
    pub loader: Option<EfiPath>,
    pub kernel: Option<EfiPath>,
    pub initrd: Option<EfiPath>,
    pub cmdline: Option<String>,
    /// Opaque id of the volume this entry was discovered on (multi-disk
    /// disambiguation). `None` for firmware-variable entries with no volume.
    pub volume: Option<String>,
    pub health: Health,
    pub last_result: Option<BootResult>,
}

impl BootEntry {
    /// Construct an entry, deriving the stable id from its durable attributes.
    pub fn new(
        kind: OsKind,
        title: impl Into<String>,
        role: EntryRole,
        method: BootMethod,
    ) -> Self {
        let id = EntryId::derive(kind, &role, None);
        BootEntry {
            id,
            title: title.into(),
            role,
            method,
            loader: None,
            kernel: None,
            initrd: None,
            cmdline: None,
            volume: None,
            health: Health::Unknown,
            last_result: None,
        }
    }
    /// Recompute the stable id from the current durable attributes, INCLUDING the
    /// volume (call after setting loader/volume). Keeps the id as the SSOT key.
    pub fn rederive_id(&mut self, kind: OsKind) {
        self.id = EntryId::derive_scoped(
            kind,
            &self.role,
            self.loader.as_ref().map(|p| p.0.as_str()),
            self.volume.as_deref(),
        );
    }
    /// Attach a loader path and re-derive the id to include it (SSOT convergence).
    pub fn with_loader(mut self, kind: OsKind, p: EfiPath) -> Self {
        self.id = EntryId::derive(kind, &self.role, Some(&p.0));
        self.loader = Some(p);
        self
    }
    pub fn bootable(&self) -> bool {
        self.health.is_bootable()
    }
}
