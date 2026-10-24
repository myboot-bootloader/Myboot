//! graph — the Boot Graph: MyBoot's single source of truth for what is bootable
//! (report §3.5, Figure 3.2; ADR 0003).
//!
//! The domain core of the hexagonal architecture: ZERO knowledge of uefi-rs,
//! filesystems, or partition formats — pure data + invariants, unit-tested on the
//! host. Derived state, rebuilt each boot from discovery, never persisted (§3.9).
//!
//! Split into cohesive modules (SRP): `ids` (stable identifiers), `kinds` (the
//! OS/role/method enums that feed those ids), `health` (health + outcomes),
//! `entry` (a bootable leaf), and `tree` (the disks→OS→entries container). All
//! types are re-exported here so consumers use a flat `graph::X` path (SSOT for
//! the public surface).
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

mod entry;
mod health;
mod ids;
mod kinds;
mod tree;

pub use entry::{BootEntry, EfiPath};
pub use health::{BootResult, Health, Reason};
pub use ids::{DiskId, EntryId};
pub use kinds::{BootMethod, EntryRole, OsKind};
pub use tree::{BootGraph, DiskNode, OsNode};

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;

    fn gen(kind: OsKind, g: u32) -> BootEntry {
        BootEntry::new(
            kind,
            format!("Generation {g}"),
            EntryRole::Generation(g),
            BootMethod::NixGeneration,
        )
    }

    #[test]
    fn entry_id_is_stable_and_order_independent() {
        let a = EntryId::derive(
            OsKind::NixOs,
            &EntryRole::Generation(128),
            Some("/EFI/nixos/x.efi"),
        );
        let b = EntryId::derive(
            OsKind::NixOs,
            &EntryRole::Generation(128),
            Some("\\EFI\\nixos\\X.efi"),
        );
        assert_eq!(a, b, "path separators/case must normalise to a stable id");
        let c = EntryId::derive(
            OsKind::NixOs,
            &EntryRole::Generation(127),
            Some("/EFI/nixos/x.efi"),
        );
        assert_ne!(a, c, "different generation must yield a different id");
    }

    #[test]
    fn health_gates_bootability() {
        assert!(Health::Healthy.is_bootable());
        assert!(Health::Degraded(Reason::new("kernel changed")).is_bootable());
        assert!(!Health::Unbootable(Reason::new("missing kernel")).is_bootable());
        assert!(!Health::Unknown.is_bootable());
        assert!(Health::Healthy.rank() > Health::Degraded(Reason::new("x")).rank());
        assert!(Health::Unknown.rank() > Health::Unbootable(Reason::new("x")).rank());
    }

    #[test]
    fn find_and_annotate_by_stable_id() {
        let mut g = BootGraph::new();
        let mut os = OsNode::new(OsKind::NixOs, "NixOS");
        os.push(gen(OsKind::NixOs, 128));
        os.push(gen(OsKind::NixOs, 127));
        g.add_disk(DiskNode {
            id: DiskId(0),
            label: "NVMe0".to_string(),
            systems: alloc::vec![os],
        });

        let id = EntryId::derive(OsKind::NixOs, &EntryRole::Generation(128), None);
        assert!(g.find(&id).is_some());
        g.find_mut(&id).unwrap().health = Health::Healthy;
        assert_eq!(g.find(&id).unwrap().health, Health::Healthy);
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn canonical_order_puts_newest_generation_first() {
        let mut g = BootGraph::new();
        let mut os = OsNode::new(OsKind::NixOs, "NixOS");
        os.push(gen(OsKind::NixOs, 126));
        os.push(gen(OsKind::NixOs, 128));
        os.push(gen(OsKind::NixOs, 127));
        g.add_disk(DiskNode {
            id: DiskId(0),
            label: "d".to_string(),
            systems: alloc::vec![os],
        });
        g.sort_canonical();
        let gens: alloc::vec::Vec<_> = g.disks[0].systems[0]
            .entries
            .iter()
            .map(|e| match &e.role {
                EntryRole::Generation(x) => *x,
                _ => 0,
            })
            .collect();
        assert_eq!(gens, alloc::vec![128, 127, 126]);
    }

    #[test]
    fn bootable_filter_excludes_unbootable() {
        let mut g = BootGraph::new();
        let mut os = OsNode::new(OsKind::NixOs, "NixOS");
        let mut good = gen(OsKind::NixOs, 128);
        good.health = Health::Healthy;
        let mut bad = gen(OsKind::NixOs, 127);
        bad.health = Health::Unbootable(Reason::new("x"));
        os.push(good);
        os.push(bad);
        g.add_disk(DiskNode {
            id: DiskId(0),
            label: "d".to_string(),
            systems: alloc::vec![os],
        });
        assert_eq!(g.bootable().count(), 1);
    }

    #[test]
    fn retain_entries_drops_matched_and_prunes_empty_nodes() {
        let mut g = BootGraph::new();
        let mut a = OsNode::new(OsKind::GenericEfi, "A");
        let mut e_own = BootEntry::new(
            OsKind::GenericEfi,
            "EFI Fallback Loader",
            EntryRole::Default,
            BootMethod::EfiChainload,
        );
        e_own.loader = Some(EfiPath("\\EFI\\BOOT\\BOOTX64.EFI".to_string()));
        e_own.volume = Some("volMINE".to_string());
        a.push(e_own);
        let mut b = OsNode::new(OsKind::GenericEfi, "B");
        let mut e_iso = BootEntry::new(
            OsKind::GenericEfi,
            "EFI Fallback Loader",
            EntryRole::Default,
            BootMethod::EfiChainload,
        );
        e_iso.loader = Some(EfiPath("\\EFI\\BOOT\\BOOTX64.EFI".to_string()));
        e_iso.volume = Some("volISO".to_string());
        b.push(e_iso);
        g.add_disk(DiskNode {
            id: DiskId(0),
            label: "d".to_string(),
            systems: alloc::vec![a, b],
        });
        assert_eq!(g.len(), 2);

        // Drop only MyBoot's own (volMINE + fallback loader); the ISO's stays.
        g.retain_entries(|e| {
            let own = e.volume.as_deref() == Some("volMINE")
                && e.loader
                    .as_ref()
                    .map(|p| p.0.eq_ignore_ascii_case("\\EFI\\BOOT\\BOOTX64.EFI"))
                    .unwrap_or(false);
            !own
        });
        assert_eq!(g.len(), 1, "only MyBoot's own entry is removed");
        assert_eq!(
            g.entries().next().unwrap().volume.as_deref(),
            Some("volISO")
        );
        assert_eq!(
            g.disks[0].systems.len(),
            1,
            "the emptied OS node was pruned"
        );
    }
}
