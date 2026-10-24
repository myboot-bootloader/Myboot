//! Enumerations describing WHAT an entry is and HOW it boots (report §3.5, §3.6).
//! `OsKind::slug` and `EntryRole::write_slug` feed the stable id and must stay
//! constant for existing variants (changing them would break id stability/SSOT).
extern crate alloc;
use alloc::string::String;

/// Operating-system family.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OsKind {
    Windows,
    Linux,
    NixOs,
    Bsd,
    GenericEfi,
    Recovery,
}
impl OsKind {
    pub const fn slug(self) -> &'static str {
        match self {
            OsKind::Windows => "windows",
            OsKind::Linux => "linux",
            OsKind::NixOs => "nixos",
            OsKind::Bsd => "bsd",
            OsKind::GenericEfi => "efi",
            OsKind::Recovery => "recovery",
        }
    }
}

/// The role of an entry within its OS (report §3.5).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EntryRole {
    Default,
    Generation(u32),
    Specialisation { parent: u32, name: String },
    Kernel(String),
    Recovery,
    Fallback,
}
impl EntryRole {
    pub(crate) fn write_slug(&self, s: &mut String) {
        use core::fmt::Write;
        match self {
            EntryRole::Default => {
                s.push_str("default");
            }
            EntryRole::Generation(g) => {
                let _ = write!(s, "gen{g}");
            }
            EntryRole::Specialisation { parent, name } => {
                let _ = write!(s, "gen{parent}.{name}");
            }
            EntryRole::Kernel(k) => {
                s.push_str("kernel.");
                s.push_str(k);
            }
            EntryRole::Recovery => {
                s.push_str("recovery");
            }
            EntryRole::Fallback => {
                s.push_str("fallback");
            }
        }
    }
    /// Sort key: lower sorts first (current generation before older, etc.).
    pub(crate) fn order_key(&self) -> (u8, i64) {
        match self {
            EntryRole::Default => (0, 0),
            EntryRole::Generation(g) => (1, -(*g as i64)),
            EntryRole::Specialisation { parent, .. } => (2, -(*parent as i64)),
            EntryRole::Kernel(_) => (3, 0),
            EntryRole::Recovery => (4, 0),
            EntryRole::Fallback => (5, 0),
        }
    }
}

/// How an entry is launched (report §3.6). Chainloading uses firmware LoadImage;
/// only `LinuxDirect` uses the `arch` assembly handoff.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootMethod {
    WindowsBootManager,
    LinuxEfiStub,
    LinuxDirect,
    NixGeneration,
    EfiChainload,
}
