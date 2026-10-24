//! Generic EFI provider (report §3.6). Chainloads any other native `.efi`
//! loader — the catch-all so unknown-but-valid EFI applications remain bootable.
extern crate alloc;
use crate::{health_from_required, not_launchable, BootProvider, LaunchPlan, PlanError};
use graph::{BootEntry, BootMethod, EntryRole, Health, OsKind};

pub struct Generic;

impl BootProvider for Generic {
    fn kind(&self) -> OsKind {
        OsKind::GenericEfi
    }
    fn handles(&self, entry: &BootEntry) -> bool {
        entry.method == BootMethod::EfiChainload && !matches!(entry.role, EntryRole::Recovery)
    }
    fn validate(&self, entry: &BootEntry, present: &dyn Fn(&str) -> bool) -> Health {
        match &entry.loader {
            Some(p) => health_from_required(p, present, "EFI image"),
            None => Health::Unbootable(graph::Reason::new("no EFI image path")),
        }
    }
    fn plan(&self, entry: &BootEntry) -> Result<LaunchPlan, PlanError> {
        let image = entry
            .loader
            .clone()
            .ok_or_else(|| not_launchable("no EFI image path"))?;
        Ok(LaunchPlan::Chainload {
            image,
            options: None,
            initrd: None,
        })
    }
}
