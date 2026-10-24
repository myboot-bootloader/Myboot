//! The Boot Graph container: disks → OS → entries (report §3.5, Figure 3.2).
//! The single authoritative model of what is bootable (SSOT); all interfaces and
//! decisions operate over this.
extern crate alloc;
use crate::entry::BootEntry;
use crate::ids::{DiskId, EntryId};
use crate::kinds::OsKind;
use alloc::string::String;
use alloc::vec::Vec;

/// An operating system installed on a disk, grouping its entries.
#[derive(Clone, Debug)]
pub struct OsNode {
    pub kind: OsKind,
    pub name: String,
    pub entries: Vec<BootEntry>,
}
impl OsNode {
    pub fn new(kind: OsKind, name: impl Into<String>) -> Self {
        OsNode {
            kind,
            name: name.into(),
            entries: Vec::new(),
        }
    }
    pub fn push(&mut self, e: BootEntry) {
        self.entries.push(e);
    }
}

/// A physical disk holding one or more operating systems.
#[derive(Clone, Debug)]
pub struct DiskNode {
    pub id: DiskId,
    pub label: String,
    pub systems: Vec<OsNode>,
}

/// The Boot Graph: disks → OS → entries (SSOT).
#[derive(Clone, Debug, Default)]
pub struct BootGraph {
    pub disks: Vec<DiskNode>,
}

impl BootGraph {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_disk(&mut self, disk: DiskNode) {
        self.disks.push(disk);
    }

    /// Iterate every entry in the graph, flattened, in traversal order.
    pub fn entries(&self) -> impl Iterator<Item = &BootEntry> {
        self.disks
            .iter()
            .flat_map(|d| d.systems.iter())
            .flat_map(|o| o.entries.iter())
    }

    /// Look up an entry by its stable id (SSOT lookups go through here).
    pub fn find(&self, id: &EntryId) -> Option<&BootEntry> {
        self.entries().find(|e| &e.id == id)
    }

    /// Mutable lookup for annotating health / results.
    pub fn find_mut(&mut self, id: &EntryId) -> Option<&mut BootEntry> {
        self.disks
            .iter_mut()
            .flat_map(|d| d.systems.iter_mut())
            .flat_map(|o| o.entries.iter_mut())
            .find(|e| &e.id == id)
    }

    pub fn len(&self) -> usize {
        self.entries().count()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Keep only entries for which `keep` returns true, pruning any OS or disk node
    /// left empty. This preserves the SSOT structure and the full n-OS listing —
    /// only the matched entries are dropped. Used, e.g., to remove MyBoot's OWN
    /// loader so it never offers to boot itself.
    pub fn retain_entries<F: FnMut(&BootEntry) -> bool>(&mut self, mut keep: F) {
        for d in &mut self.disks {
            for o in &mut d.systems {
                o.entries.retain(|e| keep(e));
            }
            d.systems.retain(|o| !o.entries.is_empty());
        }
        self.disks.retain(|d| !d.systems.is_empty());
    }

    /// Every currently bootable entry (health-permitting).
    pub fn bootable(&self) -> impl Iterator<Item = &BootEntry> {
        self.entries().filter(|e| e.bootable())
    }

    /// Order entries within each OS canonically (current generation first, etc.).
    /// Deterministic — same graph always orders the same way (SSOT stability).
    pub fn sort_canonical(&mut self) {
        for d in &mut self.disks {
            for o in &mut d.systems {
                o.entries.sort_by(|a, b| {
                    a.role
                        .order_key()
                        .cmp(&b.role.order_key())
                        .then_with(|| a.title.cmp(&b.title))
                });
            }
        }
    }
}
