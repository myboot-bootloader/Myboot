//! The provider registry — the microkernel's plug-in table (ADR 0003). Given an
//! entry, find the one provider that handles it. Order matters only for entries a
//! single provider claims; each entry is claimed by exactly one provider here.
extern crate alloc;
use crate::{
    generic::Generic, linux::Linux, nixos::NixOs, recovery::Recovery, windows::Windows,
    BootProvider,
};
use alloc::boxed::Box;
use alloc::vec::Vec;
use graph::BootEntry;

/// Holds the built-in providers. New OS support = push another provider here.
pub struct Registry {
    providers: Vec<Box<dyn BootProvider>>,
}

impl Registry {
    /// The default set of providers, in resolution order (most specific first).
    pub fn with_builtins() -> Self {
        Registry {
            providers: alloc::vec![
                Box::new(Windows) as Box<dyn BootProvider>,
                Box::new(NixOs),
                Box::new(Linux),
                Box::new(Recovery),
                Box::new(Generic), // catch-all last
            ],
        }
    }

    /// Find the provider that handles `entry`, if any.
    pub fn resolve(&self, entry: &BootEntry) -> Option<&dyn BootProvider> {
        self.providers
            .iter()
            .map(|b| b.as_ref())
            .find(|p| p.handles(entry))
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

/// Convenience for callers that just want the provider for one entry.
pub fn provider_for(entry: &BootEntry) -> Option<Box<dyn BootProvider>> {
    use graph::{BootMethod, OsKind};
    // Match without allocating a Registry each time isn't necessary here; keep it
    // simple and correct.
    let _ = (OsKind::Windows, BootMethod::WindowsBootManager);
    match entry.method {
        BootMethod::WindowsBootManager => Some(Box::new(Windows)),
        BootMethod::NixGeneration => Some(Box::new(NixOs)),
        BootMethod::LinuxEfiStub | BootMethod::LinuxDirect => Some(Box::new(Linux)),
        BootMethod::EfiChainload => {
            if matches!(entry.role, graph::EntryRole::Recovery) {
                Some(Box::new(Recovery))
            } else {
                Some(Box::new(Generic))
            }
        }
    }
}
