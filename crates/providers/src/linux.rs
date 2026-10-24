//! Linux provider (report §3.6). Two launch styles:
//!   * EFI stub  — the kernel is itself an EFI image; chainload it via firmware,
//!     passing the command line as LoadOptions and the initrd via the firmware
//!     initrd-media protocol (the systemd-boot approach, Secure-Boot friendly).
//!   * Direct    — we load the kernel ourselves and hand off via `arch`
//!     (used when a kernel is not an EFI stub); gated by the security policy.
extern crate alloc;
use crate::{health_from_required, not_launchable, BootProvider, LaunchPlan, PlanError};
use graph::{BootEntry, BootMethod, Health, OsKind, Reason};

pub struct Linux;

impl BootProvider for Linux {
    fn kind(&self) -> OsKind {
        OsKind::Linux
    }
    fn handles(&self, entry: &BootEntry) -> bool {
        matches!(
            entry.method,
            BootMethod::LinuxEfiStub | BootMethod::LinuxDirect
        )
    }
    fn validate(&self, entry: &BootEntry, present: &dyn Fn(&str) -> bool) -> Health {
        let kernel = match &entry.kernel {
            Some(k) => k,
            None => return Health::Unbootable(Reason::new("no kernel path")),
        };
        match health_from_required(kernel, present, "kernel") {
            Health::Healthy => match &entry.initrd {
                // A declared-but-missing initrd degrades (bootable, but warned).
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
            .ok_or_else(|| not_launchable("no kernel path"))?;
        match entry.method {
            BootMethod::LinuxEfiStub => Ok(LaunchPlan::Chainload {
                image: kernel,
                options: entry.cmdline.clone(),
                initrd: entry.initrd.clone(),
            }),
            BootMethod::LinuxDirect => Ok(LaunchPlan::DirectKernel {
                kernel,
                initrd: entry.initrd.clone(),
                cmdline: entry.cmdline.clone(),
            }),
            _ => Err(PlanError::WrongProvider),
        }
    }
}
