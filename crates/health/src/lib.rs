//! health — validation and health scoring for Boot Graph entries
//! (report §3.5). Turns "an entry exists" into "an entry is Healthy / Degraded /
//! Unbootable, and here is the reason", which the policy engine and the UIs then
//! consume. This is where the semantic difference between a boot *selector* and a
//! boot *manager* is made concrete (report §2.6).
//!
//! Validation needs to know which loader/kernel/initrd files actually exist on
//! the ESP; it asks through the `FileStore` port, so it is host-tested with a
//! mock and has zero uefi knowledge.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

use alloc::format;
use graph::{BootGraph, BootEntry, Health, Reason, BootResult};
use ports::FileStore;

/// Validate every entry in the graph against the filesystem and its last result,
/// writing a `Health` (with reason) back into each entry. Mutates the graph in
/// place — the graph remains the single source of truth (SSOT).
pub fn assess_all<F: FileStore>(graph: &mut BootGraph, fs: &F) {
    // Collect ids first to avoid borrow conflicts, then annotate.
    let ids: alloc::vec::Vec<_> = graph.entries().map(|e| e.id.clone()).collect();
    for id in ids {
        let health = {
            let entry = graph.find(&id).expect("id came from the graph");
            assess_entry(entry, fs)
        };
        if let Some(e) = graph.find_mut(&id) { e.health = health; }
    }
}

/// Validate every entry against the volume it was DISCOVERED on. `resolve` maps
/// an entry's volume id (`None` for firmware-variable entries) to a store. This is
/// what the engine uses on a multi-volume machine: an entry whose loader/kernel
/// exists only on ANOTHER disk must not count as healthy (the "any volume" check
/// let such an entry boot the other disk's file under this entry's title).
pub fn assess_all_scoped<'a, R>(graph: &mut BootGraph, resolve: R)
where
    R: Fn(Option<&str>) -> &'a dyn FileStore,
{
    let ids: alloc::vec::Vec<_> = graph.entries().map(|e| e.id.clone()).collect();
    for id in ids {
        let health = {
            let entry = graph.find(&id).expect("id came from the graph");
            assess_entry(entry, resolve(entry.volume.as_deref()))
        };
        if let Some(e) = graph.find_mut(&id) { e.health = health; }
    }
}

/// Assess a single entry. Pure w.r.t. the entry; reads file existence via the port.
/// The rules (report §3.5):
///   * a required loader/kernel/initrd that is missing  → Unbootable(reason)
///   * a last recorded result of Failed                 → Degraded(reason)
///   * everything present and no failure                → Healthy
pub fn assess_entry<F: FileStore + ?Sized>(entry: &BootEntry, fs: &F) -> Health {
    // 1. Required files must exist.
    if let Some(p) = &entry.loader {
        if !fs.exists(&p.0) {
            return Health::Unbootable(Reason::new(format!("loader missing: {}", p.0)));
        }
    }
    if let Some(p) = &entry.kernel {
        if !fs.exists(&p.0) {
            return Health::Unbootable(Reason::new(format!("kernel missing: {}", p.0)));
        }
    }
    if let Some(p) = &entry.initrd {
        if !fs.exists(&p.0) {
            // A missing initrd is serious but sometimes recoverable → Degraded,
            // not Unbootable, so the entry still appears as a (warned) option.
            return Health::Degraded(Reason::new(format!("initrd missing: {}", p.0)));
        }
    }

    // 2. History: a previously failed boot degrades confidence.
    match entry.last_result {
        Some(BootResult::Failed) => Health::Degraded(Reason::new("previous boot did not confirm")),
        _ => Health::Healthy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph::{BootGraph, DiskNode, DiskId, OsNode, OsKind, EntryRole, BootMethod, BootEntry, EfiPath, Health, BootResult};
    use ports::mock::MockFileStore;
    use alloc::string::ToString;

    fn win_entry() -> BootEntry {
        let mut e = BootEntry::new(OsKind::Windows, "Windows 11", EntryRole::Default, BootMethod::WindowsBootManager);
        e.loader = Some(EfiPath("\\EFI\\Microsoft\\Boot\\bootmgfw.efi".to_string()));
        e
    }

    #[test]
    fn missing_loader_is_unbootable() {
        let fs = MockFileStore::new(); // empty ESP
        let h = assess_entry(&win_entry(), &fs);
        assert!(matches!(h, Health::Unbootable(_)));
        assert!(!h.is_bootable());
    }

    #[test]
    fn present_loader_is_healthy() {
        let fs = MockFileStore::new().with_file("\\EFI\\Microsoft\\Boot\\bootmgfw.efi", b"MZ");
        let h = assess_entry(&win_entry(), &fs);
        assert_eq!(h, Health::Healthy);
    }

    #[test]
    fn previous_failure_degrades_but_stays_bootable() {
        let fs = MockFileStore::new().with_file("\\EFI\\Microsoft\\Boot\\bootmgfw.efi", b"MZ");
        let mut e = win_entry();
        e.last_result = Some(BootResult::Failed);
        let h = assess_entry(&e, &fs);
        assert!(matches!(h, Health::Degraded(_)));
        assert!(h.is_bootable());
    }

    #[test]
    fn missing_initrd_is_degraded_not_unbootable() {
        let fs = MockFileStore::new()
            .with_file("\\EFI\\nixos\\kernel.efi", b"MZ"); // kernel present, initrd absent
        let mut e = BootEntry::new(OsKind::NixOs, "gen", EntryRole::Generation(1), BootMethod::LinuxDirect);
        e.kernel = Some(EfiPath("\\EFI\\nixos\\kernel.efi".to_string()));
        e.initrd = Some(EfiPath("\\EFI\\nixos\\initrd".to_string()));
        let h = assess_entry(&e, &fs);
        assert!(matches!(h, Health::Degraded(_)));
    }

    #[test]
    fn assess_all_annotates_graph_in_place() {
        let fs = MockFileStore::new().with_file("\\EFI\\Microsoft\\Boot\\bootmgfw.efi", b"MZ");
        let mut g = BootGraph::new();
        let mut os = OsNode::new(OsKind::Windows, "Windows");
        os.push(win_entry());
        g.add_disk(DiskNode { id: DiskId(0), label: "d".to_string(), systems: alloc::vec![os] });
        assess_all(&mut g, &fs);
        assert_eq!(g.entries().next().unwrap().health, Health::Healthy);
    }

    fn one_entry_graph(e: BootEntry) -> BootGraph {
        let mut os = OsNode::new(OsKind::NixOs, "NixOS");
        os.push(e);
        let mut g = BootGraph::new();
        g.add_disk(DiskNode { id: DiskId(0), label: "system".to_string(), systems: alloc::vec![os] });
        g
    }

    #[test]
    fn scoped_kernel_only_on_another_volume_is_unbootable() {
        // Disk A's generation names a kernel that exists only on disk B. Checked
        // against "any volume" this was Healthy; scoped, it is Unbootable.
        let vol_a = MockFileStore::new().with_file("\\loader\\entries\\a.conf", b"x");
        let vol_b = MockFileStore::new().with_file("\\EFI\\nixos\\k.efi", b"MZ");
        let mut e = BootEntry::new(OsKind::NixOs, "gen 5", EntryRole::Generation(5), BootMethod::NixGeneration);
        e.kernel = Some(EfiPath("\\EFI\\nixos\\k.efi".to_string()));
        e.volume = Some("volA".to_string());
        let mut g = one_entry_graph(e);
        let any: &dyn FileStore = &vol_b; // stands in for "any volume has it"
        assess_all_scoped(&mut g, |vol| match vol {
            Some("volA") => &vol_a as &dyn FileStore,
            Some("volB") => &vol_b as &dyn FileStore,
            _ => any,
        });
        assert!(matches!(g.entries().next().unwrap().health, Health::Unbootable(_)));
    }

    #[test]
    fn scoped_volumeless_entry_uses_the_fallback_store() {
        // Firmware-variable entries carry no volume; they keep the any-volume check.
        let any = MockFileStore::new().with_file("\\EFI\\Microsoft\\Boot\\bootmgfw.efi", b"MZ");
        let empty = MockFileStore::new();
        let mut g = one_entry_graph(win_entry());
        assess_all_scoped(&mut g, |vol| match vol {
            None => &any as &dyn FileStore,
            Some(_) => &empty as &dyn FileStore,
        });
        assert_eq!(g.entries().next().unwrap().health, Health::Healthy);
    }
}
