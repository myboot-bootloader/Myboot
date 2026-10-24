//! Windows provider (report §3.6). Chainloads the Windows Boot Manager. MyBoot
//! delegates to Windows' own boot chain rather than replacing it.
extern crate alloc;
use crate::{health_from_required, not_launchable, BootProvider, LaunchPlan, PlanError};
use graph::{BootEntry, BootMethod, Health, OsKind};

pub struct Windows;

impl BootProvider for Windows {
    fn kind(&self) -> OsKind {
        OsKind::Windows
    }
    fn handles(&self, entry: &BootEntry) -> bool {
        entry.method == BootMethod::WindowsBootManager
    }
    fn validate(&self, entry: &BootEntry, present: &dyn Fn(&str) -> bool) -> Health {
        match &entry.loader {
            Some(p) => health_from_required(p, present, "Windows loader"),
            None => Health::Unbootable(graph::Reason::new("no Windows loader path")),
        }
    }
    fn plan(&self, entry: &BootEntry) -> Result<LaunchPlan, PlanError> {
        let image = entry
            .loader
            .clone()
            .ok_or_else(|| not_launchable("no Windows loader path"))?;
        Ok(LaunchPlan::Chainload {
            image,
            options: None,
            initrd: None,
        })
    }
}
