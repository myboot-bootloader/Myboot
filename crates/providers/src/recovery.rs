//! Recovery provider (report §3.6, §3.10). Handles recovery-environment entries
//! (WinRE, a Linux recovery target, the UEFI shell registered as a recovery
//! option). Always chainloaded; identified by the Recovery role.
extern crate alloc;
use crate::{health_from_required, not_launchable, BootProvider, LaunchPlan, PlanError};
use graph::{BootEntry, EntryRole, Health, OsKind};

pub struct Recovery;

impl BootProvider for Recovery {
    fn kind(&self) -> OsKind {
        OsKind::Recovery
    }
    fn handles(&self, entry: &BootEntry) -> bool {
        matches!(entry.role, EntryRole::Recovery)
    }
    fn validate(&self, entry: &BootEntry, present: &dyn Fn(&str) -> bool) -> Health {
        match &entry.loader {
            Some(p) => health_from_required(p, present, "recovery image"),
            None => Health::Degraded(graph::Reason::new("recovery image path unknown")),
        }
    }
    fn plan(&self, entry: &BootEntry) -> Result<LaunchPlan, PlanError> {
        let image = entry
            .loader
            .clone()
            .ok_or_else(|| not_launchable("no recovery image path"))?;
        Ok(LaunchPlan::Chainload {
            image,
            options: None,
            initrd: None,
        })
    }
}
