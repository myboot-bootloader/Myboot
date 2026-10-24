//! Merge RawEntries from all sources into a single Boot Graph (report §3.5).
//! De-duplication by stable id enforces the SSOT: two sources describing the
//! same target (e.g. a firmware Boot#### and an ESP probe for Windows) collapse
//! into one entry, with the richer description winning.
extern crate alloc;
use crate::RawEntry;
use alloc::string::String;
use alloc::vec::Vec;
use graph::{BootEntry, BootGraph, DiskId, DiskNode, EfiPath, EntryId, OsKind, OsNode};

/// Build the Boot Graph from raw entries. All entries currently attach to a
/// single logical disk (the running machine's storage); multi-disk grouping is a
/// later discovery refinement and does not change the public shape.
pub fn build_graph(raw: Vec<RawEntry>) -> BootGraph {
    // 1. Convert to BootEntry (deriving stable ids), de-duplicating by id.
    let mut entries: Vec<BootEntry> = Vec::new();
    for r in raw {
        let e = to_entry(r);
        match entries.iter_mut().find(|x| x.id == e.id) {
            Some(existing) => merge_into(existing, e),
            None => entries.push(e),
        }
    }

    // 1b. Drop entries with nothing to launch. A firmware Boot#### variable can
    //     point at a whole device (not a file), leaving an entry with no loader
    //     AND no kernel after merging. No provider can build a launch plan for
    //     such an entry, so it is not bootable and must not sit in the graph —
    //     otherwise it can be picked as the default and dead-end the handoff.
    //     (Merging happens first, so an entry that gained a loader from a second
    //     source is kept.)
    entries.retain(|e| e.loader.is_some() || e.kernel.is_some());

    // 1c. A firmware-variable entry (volume = None) that duplicates a filesystem
    //     entry found ON a volume is redundant — the volume-bearing one is more
    //     specific and can be chainloaded from the right disk. Drop the volume-less
    //     twin (its id is the base of the volume-bearing one, i.e. `<base>@<vol>`).
    //     This removes NVRAM/ESP duplicates WITHOUT merging two genuinely distinct
    //     same-path loaders that live on different volumes.
    let volumed_bases: alloc::vec::Vec<alloc::string::String> = entries
        .iter()
        .filter(|e| e.volume.is_some())
        .filter_map(|e| {
            e.id.as_str()
                .split('@')
                .next()
                .map(alloc::string::String::from)
        })
        .collect();
    entries.retain(|e| e.volume.is_some() || !volumed_bases.iter().any(|b| b == e.id.as_str()));

    // 2. Group entries by OS family, preserving first-seen order.
    let mut systems: Vec<OsNode> = Vec::new();
    for e in entries {
        let kind = os_kind_of(&e.id).unwrap_or(OsKind::GenericEfi);
        match systems.iter_mut().find(|o| o.kind == kind) {
            Some(os) => os.push(e),
            None => {
                let mut os = OsNode::new(kind, os_display_name(kind));
                os.push(e);
                systems.push(os);
            }
        }
    }

    let mut graph = BootGraph::new();
    graph.add_disk(DiskNode {
        id: DiskId(0),
        label: String::from("system"),
        systems,
    });
    graph.sort_canonical();
    graph
}

fn to_entry(r: RawEntry) -> BootEntry {
    let mut e = BootEntry::new(r.kind, r.title, r.role, r.method);
    e.volume = r.volume;
    if let Some(l) = r.loader {
        e.loader = Some(EfiPath(l));
    }
    e.kernel = r.kernel.map(EfiPath);
    e.initrd = r.initrd.map(EfiPath);
    e.cmdline = r.cmdline;
    // Distinguish otherwise-identical generic fallback loaders (e.g. one per ISO
    // on separate volumes) by tagging the title with a short volume id, so the
    // n-OS menu shows distinct entries the user can tell apart.
    if e.title.contains("Fallback Loader") {
        if let Some(v) = &e.volume {
            let tag = &v[v.len().saturating_sub(6)..];
            e.title = alloc::format!("{} [{}]", e.title, tag);
        }
    }
    // Re-derive the id from ALL durable attributes, now including the volume, so
    // the same loader path on two volumes yields two distinct entries.
    e.rederive_id(r.kind);
    e
}

/// When two sources describe the same id, keep any concrete paths and the more
/// descriptive title (longer, non-generic) — the richer evidence wins.
fn merge_into(existing: &mut BootEntry, other: BootEntry) {
    if existing.loader.is_none() {
        existing.loader = other.loader;
    }
    if existing.kernel.is_none() {
        existing.kernel = other.kernel;
    }
    if existing.initrd.is_none() {
        existing.initrd = other.initrd;
    }
    if existing.cmdline.is_none() {
        existing.cmdline = other.cmdline;
    }
    if other.title.len() > existing.title.len() {
        existing.title = other.title;
    }
}

/// Recover the OsKind from a stable id's slug prefix (ids start `<kind>:`).
fn os_kind_of(id: &EntryId) -> Option<OsKind> {
    let slug = id.as_str().split(':').next()?;
    Some(match slug {
        "windows" => OsKind::Windows,
        "linux" => OsKind::Linux,
        "nixos" => OsKind::NixOs,
        "bsd" => OsKind::Bsd,
        "recovery" => OsKind::Recovery,
        _ => OsKind::GenericEfi,
    })
}

fn os_display_name(kind: OsKind) -> &'static str {
    match kind {
        OsKind::Windows => "Windows",
        OsKind::Linux => "Linux",
        OsKind::NixOs => "NixOS",
        OsKind::Bsd => "BSD",
        OsKind::GenericEfi => "EFI",
        OsKind::Recovery => "Recovery",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph::{BootMethod, EntryRole, OsKind};

    fn raw(
        kind: OsKind,
        title: &str,
        role: EntryRole,
        method: BootMethod,
        loader: Option<&str>,
    ) -> RawEntry {
        let mut r = RawEntry::new(kind, "os", title, role, method);
        r.loader = loader.map(Into::into);
        // A real launchable entry has a loader OR a kernel; give loader-less test
        // entries a kernel (as real NixOS generations always have) so they model
        // reality and survive build_graph's un-launchable filter.
        if r.loader.is_none() {
            r.kernel = Some("\\EFI\\nixos\\kernel.efi".into());
        }
        r
    }

    #[test]
    fn groups_and_orders_by_os_and_generation() {
        let raws = alloc::vec![
            raw(
                OsKind::NixOs,
                "gen126",
                EntryRole::Generation(126),
                BootMethod::NixGeneration,
                None
            ),
            raw(
                OsKind::Windows,
                "Windows Boot Manager",
                EntryRole::Default,
                BootMethod::WindowsBootManager,
                Some("\\EFI\\Microsoft\\Boot\\bootmgfw.efi")
            ),
            raw(
                OsKind::NixOs,
                "gen128",
                EntryRole::Generation(128),
                BootMethod::NixGeneration,
                None
            ),
        ];
        let g = build_graph(raws);
        assert_eq!(g.len(), 3);
        // NixOS entries ordered newest-first within their OS node
        let nix = g.disks[0]
            .systems
            .iter()
            .find(|o| o.kind == OsKind::NixOs)
            .unwrap();
        let gens: Vec<u32> = nix
            .entries
            .iter()
            .filter_map(|e| match &e.role {
                EntryRole::Generation(x) => Some(*x),
                _ => None,
            })
            .collect();
        assert_eq!(gens, alloc::vec![128, 126]);
    }

    #[test]
    fn windows_from_two_sources_collapses_via_canonical_loader() {
        // Firmware option and ESP probe both reference the canonical loader path,
        // so they derive the SAME stable id and collapse into one entry.
        let loader = "\\EFI\\Microsoft\\Boot\\bootmgfw.efi";
        let from_vars = raw(
            OsKind::Windows,
            "Windows Boot Manager",
            EntryRole::Default,
            BootMethod::WindowsBootManager,
            Some(loader),
        );
        let from_esp = raw(
            OsKind::Windows,
            "Windows Boot Manager",
            EntryRole::Default,
            BootMethod::WindowsBootManager,
            Some(loader),
        );
        let g = build_graph(alloc::vec![from_vars, from_esp]);
        assert_eq!(g.len(), 1, "same id must collapse to one entry");
        assert!(g.entries().next().unwrap().loader.is_some());
    }

    #[test]
    fn nixos_generation_merges_partial_info_from_two_sources() {
        // Realistic merge: one source knows the kernel, another the initrd, same
        // generation (loader is None for NixOS, so id = nixos:gen128 in both).
        let mut a = raw(
            OsKind::NixOs,
            "gen128",
            EntryRole::Generation(128),
            BootMethod::NixGeneration,
            None,
        );
        a.kernel = Some("\\EFI\\nixos\\k\\bzImage".into());
        let mut b = raw(
            OsKind::NixOs,
            "NixOS 24.05 (gen 128)",
            EntryRole::Generation(128),
            BootMethod::NixGeneration,
            None,
        );
        b.initrd = Some("\\EFI\\nixos\\k\\initrd".into());
        let g = build_graph(alloc::vec![a, b]);
        assert_eq!(g.len(), 1, "same generation collapses to one entry");
        let e = g.entries().next().unwrap();
        assert!(
            e.kernel.is_some() && e.initrd.is_some(),
            "partial info merged"
        );
        assert!(e.title.contains("24.05"), "richer title wins");
    }

    #[test]
    fn entry_with_no_loader_and_no_kernel_is_dropped() {
        // A firmware Boot#### that points at a whole device yields an entry with
        // neither a loader nor a kernel — it cannot be launched and must not sit
        // in the graph as bootable.
        let mut orphan = RawEntry::new(
            OsKind::GenericEfi,
            "os",
            "UEFI HARDDISK",
            EntryRole::Default,
            BootMethod::EfiChainload,
        );
        orphan.loader = None; // and no kernel
        let win = raw(
            OsKind::Windows,
            "Windows Boot Manager",
            EntryRole::Default,
            BootMethod::WindowsBootManager,
            Some("\\EFI\\Microsoft\\Boot\\bootmgfw.efi"),
        );
        let g = build_graph(alloc::vec![orphan, win]);
        assert_eq!(g.len(), 1, "the loader-less, kernel-less entry is dropped");
        assert_eq!(g.entries().next().unwrap().title, "Windows Boot Manager");
    }

    #[test]
    fn same_loader_path_on_two_volumes_stays_distinct() {
        // The core multi-disk case: \EFI\BOOT\BOOTX64.EFI on two different volumes
        // must NOT collapse — otherwise MyBoot can't tell its own medium from a
        // Ventoy USB / an installer ISO.
        let mut a = RawEntry::new(
            OsKind::GenericEfi,
            "os",
            "EFI Fallback A",
            EntryRole::Default,
            BootMethod::EfiChainload,
        );
        a.loader = Some("\\EFI\\BOOT\\BOOTX64.EFI".into());
        a.volume = Some("volA".into());
        let mut b = RawEntry::new(
            OsKind::GenericEfi,
            "os",
            "EFI Fallback B",
            EntryRole::Default,
            BootMethod::EfiChainload,
        );
        b.loader = Some("\\EFI\\BOOT\\BOOTX64.EFI".into());
        b.volume = Some("volB".into());
        let g = build_graph(alloc::vec![a, b]);
        assert_eq!(
            g.len(),
            2,
            "same loader path on two volumes must be two entries"
        );
    }

    #[test]
    fn volumeless_twin_is_dropped_in_favor_of_volume_bearing() {
        // An NVRAM (volume=None) entry duplicating a filesystem entry found on a
        // volume is dropped; the volume-bearing one (chainloadable from its disk)
        // wins.
        let mut nvram = RawEntry::new(
            OsKind::Windows,
            "Windows",
            "Windows Boot Manager",
            EntryRole::Default,
            BootMethod::WindowsBootManager,
        );
        nvram.loader = Some("\\EFI\\Microsoft\\Boot\\bootmgfw.efi".into()); // volume None
        let mut esp = RawEntry::new(
            OsKind::Windows,
            "Windows",
            "Windows Boot Manager",
            EntryRole::Default,
            BootMethod::WindowsBootManager,
        );
        esp.loader = Some("\\EFI\\Microsoft\\Boot\\bootmgfw.efi".into());
        esp.volume = Some("volA".into());
        let g = build_graph(alloc::vec![nvram, esp]);
        assert_eq!(g.len(), 1, "the volume-less NVRAM twin is dropped");
        assert!(
            g.entries().next().unwrap().volume.is_some(),
            "the surviving entry is volume-bearing"
        );
    }

    #[test]
    fn fallback_titles_are_tagged_by_volume_for_the_n_os_menu() {
        let mut a = RawEntry::new(
            OsKind::GenericEfi,
            "os",
            "EFI Fallback Loader",
            EntryRole::Default,
            BootMethod::EfiChainload,
        );
        a.loader = Some("\\EFI\\BOOT\\BOOTX64.EFI".into());
        a.volume = Some("vol000000aaaaaa".into());
        let mut b = RawEntry::new(
            OsKind::GenericEfi,
            "os",
            "EFI Fallback Loader",
            EntryRole::Default,
            BootMethod::EfiChainload,
        );
        b.loader = Some("\\EFI\\BOOT\\BOOTX64.EFI".into());
        b.volume = Some("vol000000bbbbbb".into());
        let g = build_graph(alloc::vec![a, b]);
        let titles: alloc::vec::Vec<_> = g.entries().map(|e| e.title.clone()).collect();
        assert_eq!(g.len(), 2);
        assert!(
            titles.iter().any(|t| t.ends_with("[aaaaaa]")),
            "titles: {titles:?}"
        );
        assert!(
            titles.iter().any(|t| t.ends_with("[bbbbbb]")),
            "titles: {titles:?}"
        );
    }
}
