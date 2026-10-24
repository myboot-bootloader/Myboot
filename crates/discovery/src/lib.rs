//! discovery — reduce the messy real world (disks, firmware variables, loader
//! entries, NixOS generations) to a single uniform Boot Graph (report §3.3, §4.7).
//!
//! This is pure orchestration over the `storage` parsers and the `ports` I/O
//! abstractions, so it is fully host-tested with mocks. Each source module has
//! ONE cohesive responsibility (SRP): turn one kind of on-disk/firmware evidence
//! into a `RawEntry`. `lib.rs` merges the raw entries, de-duplicates them by
//! stable id (SSOT), and groups them into the graph.
//!
//! Sources mirror established tools:
//!   * `variables` — existing UEFI Boot#### options (like `efibootmgr`).
//!   * `esp`       — well-known loaders on the ESP (like rEFInd's scan).
//!   * `bls`       — Boot Loader Specification Type #1 entries (like systemd-boot).
//!   * `bootspec`  — NixOS bootspec generations (RFC 0125, like Lanzaboote).
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use graph::{BootMethod, EntryRole, OsKind};

pub mod bls;
pub mod bootspec;
mod build;
pub mod distros;
pub mod esp;
pub mod json;
pub mod refine;
pub mod sbat;
pub mod variables;

#[cfg(test)]
extern crate std;

/// An intermediate, source-agnostic description of one bootable thing. Sources
/// produce these; `build` turns them into `graph::BootEntry`s with stable ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEntry {
    pub kind: OsKind,
    pub os_name: String,
    pub title: String,
    pub role: EntryRole,
    pub method: BootMethod,
    pub loader: Option<String>,
    pub kernel: Option<String>,
    pub initrd: Option<String>,
    pub cmdline: Option<String>,
    /// Opaque id of the volume this entry came from. Filesystem sources leave this
    /// `None`; `discover_multi` tags each entry with the volume it was scanned on.
    pub volume: Option<String>,
}

impl RawEntry {
    pub fn new(
        kind: OsKind,
        os_name: impl Into<String>,
        title: impl Into<String>,
        role: EntryRole,
        method: BootMethod,
    ) -> Self {
        RawEntry {
            kind,
            os_name: os_name.into(),
            title: title.into(),
            role,
            method,
            loader: None,
            kernel: None,
            initrd: None,
            cmdline: None,
            volume: None,
        }
    }
}

pub use build::build_graph;

/// Run every source against the given firmware variables and ESP file store,
/// and assemble the complete Boot Graph. This is the single-volume call (kept for
/// callers/tests that have one store); entries get `volume = None`.
pub fn discover<V: ports::VarStore, F: ports::FileStore>(vars: &V, esp_fs: &F) -> graph::BootGraph {
    let mut raw: Vec<RawEntry> = Vec::new();
    raw.extend(variables::discover(vars));
    raw.extend(scan_volume(esp_fs, None));
    build_graph(raw)
}

/// Run the FILESYSTEM sources against one volume, tagging every entry with
/// `volume_id`. (Firmware-variable entries are not per-volume; see `discover_multi`.)
pub fn scan_volume<F: ports::FileStore + ?Sized>(fs: &F, volume_id: Option<&str>) -> Vec<RawEntry> {
    let mut raw: Vec<RawEntry> = Vec::new();
    raw.extend(esp::discover(fs));
    raw.extend(distros::discover(fs));
    raw.extend(bls::discover(fs));
    raw.extend(bootspec::discover(fs));
    let mut raw = refine::refine_methods(raw, fs);
    if let Some(v) = volume_id {
        for r in &mut raw {
            r.volume = Some(alloc::string::String::from(v));
        }
    }
    raw
}

/// Scan firmware variables once, then EVERY volume (all disks/ESPs), tagging each
/// filesystem entry with the volume it came from, and assemble one Boot Graph.
/// This is the multi-disk discovery the engine uses: two volumes that share a
/// loader path stay DISTINCT, and each entry knows which volume to boot from.
pub fn discover_multi<V: ports::VarStore>(
    vars: &V,
    volumes: &[(alloc::string::String, &dyn ports::FileStore)],
) -> graph::BootGraph {
    let mut raw: Vec<RawEntry> = Vec::new();
    raw.extend(variables::discover(vars));
    for (id, fs) in volumes {
        raw.extend(scan_volume(*fs, Some(id.as_str())));
    }
    build_graph(raw)
}
