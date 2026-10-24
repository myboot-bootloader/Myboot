//! NixOS provider (report §3.6, and the project's headline feature: generations).
//! A generation boots its kernel as an EFI stub with its initrd and kernel
//! params — chainloaded via firmware (Secure-Boot friendly, matching lanzaboote's
//! direction). The generation/specialisation structure is already captured in the
//! entry's role by discovery; this provider only turns it into a launch plan.
extern crate alloc;
use crate::{health_from_required, not_launchable, BootProvider, LaunchPlan, PlanError};
use graph::{BootEntry, BootMethod, Health, OsKind, Reason};

pub struct NixOs;

impl BootProvider for NixOs {
    fn kind(&self) -> OsKind {
        OsKind::NixOs
    }
    fn handles(&self, entry: &BootEntry) -> bool {
        entry.method == BootMethod::NixGeneration
    }
    fn validate(&self, entry: &BootEntry, present: &dyn Fn(&str) -> bool) -> Health {
        let kernel = match &entry.kernel {
            Some(k) => k,
            None => return Health::Unbootable(Reason::new("generation has no kernel")),
        };
        match health_from_required(kernel, present, "kernel") {
            Health::Healthy => match &entry.initrd {
                Some(i) if !present(&i.0) => {
                    Health::Degraded(Reason::new(alloc::format!("initrd missing: {}", i.0)))
                }
                _ => Health::Healthy,
            },
            other => other,
        }
    }
    fn plan(&self, entry: &BootEntry) -> Result<LaunchPlan, PlanError> {
        let kernel = entry
            .kernel
            .clone()
            .ok_or_else(|| not_launchable("generation has no kernel"))?;
        Ok(LaunchPlan::Chainload {
            image: kernel,
            options: entry.cmdline.clone(),
            initrd: entry.initrd.clone(),
        })
    }
}
