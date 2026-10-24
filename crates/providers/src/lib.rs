//! providers — the OS-adapter architecture (report §3.6; ADR 0003 microkernel
//! plug-ins). Each provider encapsulates everything OS-specific behind ONE trait,
//! so adding an OS means adding a provider, never editing the core. A provider
//! knows nothing about any other provider.
//!
//! Launching is a firmware action expressed as a `LaunchPlan` the engine hands to
//! the `platform` adapter — so the providers' decision logic stays pure and
//! host-tested, while the irreversible `LoadImage`/`StartImage`/ExitBootServices
//! happens in one audited place.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

use alloc::string::String;
use graph::{BootEntry, EfiPath, Health, OsKind, Reason};

pub mod generic;
pub mod linux;
pub mod nixos;
pub mod recovery;
mod registry;
pub mod windows;

#[cfg(test)]
extern crate std;

pub use registry::{provider_for, Registry};

/// A firmware-agnostic description of how to launch an entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchPlan {
    /// Load and start an EFI image via firmware (which verifies it under Secure
    /// Boot). `options` are passed as EFI LoadOptions (the kernel command line for
    /// EFI-stub / unified kernels); `initrd`, when present, is provided to the
    /// image via the firmware initrd-media protocol, exactly as systemd-boot does.
    Chainload {
        image: EfiPath,
        options: Option<String>,
        initrd: Option<EfiPath>,
    },
    /// Directly load a kernel (+ optional initrd) and hand off via `arch`. Used
    /// only for `LinuxDirect`; gated by the `security` launch policy.
    DirectKernel {
        kernel: EfiPath,
        initrd: Option<EfiPath>,
        cmdline: Option<String>,
    },
}

/// Why a launch plan could not be produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    NotLaunchable(Reason),
    WrongProvider,
}

/// The OS-adapter interface (report §3.6). One provider = one OS family (SRP).
pub trait BootProvider {
    fn kind(&self) -> OsKind;
    /// True if this provider recognises and can launch the entry.
    fn handles(&self, entry: &BootEntry) -> bool;
    /// Assess health given which of the entry's files exist. File existence is
    /// resolved by the caller (via the FileStore port) and passed in, so the
    /// provider stays pure.
    fn validate(&self, entry: &BootEntry, files_present: &dyn Fn(&str) -> bool) -> Health;
    /// Produce the launch plan for an entry this provider handles.
    fn plan(&self, entry: &BootEntry) -> Result<LaunchPlan, PlanError>;
}

// ---- shared helpers (kept here to avoid per-provider duplication) ----

pub(crate) fn health_from_required(
    path: &EfiPath,
    present: &dyn Fn(&str) -> bool,
    what: &str,
) -> Health {
    if present(&path.0) {
        Health::Healthy
    } else {
        Health::Unbootable(Reason::new(alloc::format!("{what} missing: {}", path.0)))
    }
}

pub(crate) fn not_launchable(msg: &str) -> PlanError {
    PlanError::NotLaunchable(Reason::new(alloc::string::ToString::to_string(msg)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use graph::{BootEntry, BootMethod, EfiPath, EntryRole, OsKind};

    fn win() -> BootEntry {
        BootEntry::new(
            OsKind::Windows,
            "Windows",
            EntryRole::Default,
            BootMethod::WindowsBootManager,
        )
        .with_loader(
            OsKind::Windows,
            EfiPath("\\EFI\\Microsoft\\Boot\\bootmgfw.efi".to_string()),
        )
    }
    fn nixgen() -> BootEntry {
        let mut e = BootEntry::new(
            OsKind::NixOs,
            "gen128",
            EntryRole::Generation(128),
            BootMethod::NixGeneration,
        );
        e.kernel = Some(EfiPath("\\EFI\\nixos\\bzImage".to_string()));
        e.initrd = Some(EfiPath("\\EFI\\nixos\\initrd".to_string()));
        e.cmdline = Some("init=/nix/init loglevel=4".to_string());
        e
    }
    fn recov() -> BootEntry {
        BootEntry::new(
            OsKind::Recovery,
            "WinRE",
            EntryRole::Recovery,
            BootMethod::EfiChainload,
        )
        .with_loader(
            OsKind::Recovery,
            EfiPath("\\EFI\\Microsoft\\Recovery\\bootmgfw.efi".to_string()),
        )
    }

    #[test]
    fn registry_routes_each_entry_to_one_provider() {
        let reg = Registry::with_builtins();
        assert_eq!(reg.resolve(&win()).unwrap().kind(), OsKind::Windows);
        assert_eq!(reg.resolve(&nixgen()).unwrap().kind(), OsKind::NixOs);
        assert_eq!(reg.resolve(&recov()).unwrap().kind(), OsKind::Recovery);
    }

    #[test]
    fn windows_plans_a_chainload() {
        let p = provider_for(&win()).unwrap();
        assert_eq!(
            p.plan(&win()).unwrap(),
            LaunchPlan::Chainload {
                image: EfiPath("\\EFI\\Microsoft\\Boot\\bootmgfw.efi".to_string()),
                options: None,
                initrd: None
            }
        );
    }

    #[test]
    fn nixos_generation_chainloads_kernel_with_cmdline_and_initrd() {
        let e = nixgen();
        let p = provider_for(&e).unwrap();
        match p.plan(&e).unwrap() {
            LaunchPlan::Chainload {
                image,
                options,
                initrd,
            } => {
                assert_eq!(image.0, "\\EFI\\nixos\\bzImage");
                assert_eq!(options.as_deref(), Some("init=/nix/init loglevel=4"));
                assert_eq!(initrd.unwrap().0, "\\EFI\\nixos\\initrd");
            }
            other => panic!("expected chainload, got {other:?}"),
        }
    }

    #[test]
    fn linux_direct_produces_direct_kernel_plan() {
        let mut e = BootEntry::new(
            OsKind::Linux,
            "linux",
            EntryRole::Default,
            BootMethod::LinuxDirect,
        );
        e.kernel = Some(EfiPath("\\vmlinuz".to_string()));
        e.initrd = Some(EfiPath("\\initrd".to_string()));
        let p = provider_for(&e).unwrap();
        assert!(matches!(
            p.plan(&e).unwrap(),
            LaunchPlan::DirectKernel { .. }
        ));
    }

    #[test]
    fn validate_flags_missing_kernel_as_unbootable() {
        let e = nixgen();
        let none_present = |_: &str| false;
        let p = provider_for(&e).unwrap();
        assert!(matches!(
            p.validate(&e, &none_present),
            Health::Unbootable(_)
        ));
        // kernel present but initrd missing → degraded
        let only_kernel = |path: &str| path == "\\EFI\\nixos\\bzImage";
        assert!(matches!(p.validate(&e, &only_kernel), Health::Degraded(_)));
    }

    #[test]
    fn missing_loader_is_not_launchable() {
        let e = BootEntry::new(
            OsKind::Windows,
            "w",
            EntryRole::Default,
            BootMethod::WindowsBootManager,
        );
        let p = provider_for(&e).unwrap();
        assert!(matches!(p.plan(&e), Err(PlanError::NotLaunchable(_))));
    }
}
