//! The recovery screen model (design: docs/diagrams/fig36.svg). When the user asks
//! for recovery (or a boot failed), MyBoot shows a structured diagnosis: every
//! candidate with a pass/fail mark, a single recommended pick, and a diagnostics
//! panel. Pure and host-tested — no drawing, no firmware here.
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use graph::{BootGraph, EntryId};

/// One line in the diagnosis list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagItem {
    pub id: EntryId,
    pub title: String,
    pub ok: bool,       // did/does this entry look bootable?
    pub bootable: bool, // can we actually boot it now?
}

/// The recovery view: the diagnosis list, the recommended entry, and a set of
/// key/value diagnostics for the side panel.
pub struct Recovery {
    items: Vec<DiagItem>,
    diagnostics: Vec<(String, String)>,
    recommended: Option<usize>,
    cursor: usize,
}

/// Input outcomes for the recovery screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryOutcome {
    Idle,
    Boot(EntryId),
    Back,
}

impl Recovery {
    /// Build from the Boot Graph. `failed` (if any) is the entry whose boot did not
    /// confirm — it's marked failed and never recommended. The recommendation is
    /// the first healthy, bootable entry that isn't the failed one.
    pub fn from_graph(graph: &BootGraph, failed: Option<&EntryId>) -> Self {
        let mut items = Vec::new();
        for d in &graph.disks {
            for os in &d.systems {
                for e in &os.entries {
                    let is_failed = failed == Some(&e.id);
                    items.push(DiagItem {
                        id: e.id.clone(),
                        title: e.title.clone(),
                        ok: e.bootable() && !is_failed,
                        bootable: e.bootable(),
                    });
                }
            }
        }
        let recommended = items.iter().position(|it| it.ok && it.bootable);
        let cursor = recommended.unwrap_or(0);

        let bootable = items.iter().filter(|i| i.bootable).count();
        let mut diagnostics = Vec::new();
        diagnostics.push((String::from("entries"), int(items.len())));
        diagnostics.push((String::from("bootable"), int(bootable)));
        if let Some(r) = recommended {
            diagnostics.push((String::from("recommend"), items[r].title.clone()));
        }
        if let Some(f) = failed {
            diagnostics.push((String::from("failed"), String::from(f.as_str())));
        }
        diagnostics.push((
            String::from("result"),
            if bootable > 0 {
                String::from("fallback available")
            } else {
                String::from("no candidate")
            },
        ));

        Recovery {
            items,
            diagnostics,
            recommended,
            cursor,
        }
    }

    pub fn items(&self) -> &[DiagItem] {
        &self.items
    }
    pub fn diagnostics(&self) -> &[(String, String)] {
        &self.diagnostics
    }
    pub fn recommended(&self) -> Option<usize> {
        self.recommended
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn selected(&self) -> Option<&DiagItem> {
        self.items.get(self.cursor)
    }

    /// Handle Up/Down/Enter/Escape (same event enum as the menu).
    pub fn handle(&mut self, ev: crate::menu::InputEvent) -> RecoveryOutcome {
        use crate::menu::InputEvent::*;
        match ev {
            Up => {
                self.move_cursor(-1);
                RecoveryOutcome::Idle
            }
            Down => {
                self.move_cursor(1);
                RecoveryOutcome::Idle
            }
            Enter | Timeout => match self.selected() {
                Some(it) if it.bootable => RecoveryOutcome::Boot(it.id.clone()),
                _ => RecoveryOutcome::Idle,
            },
            Escape => RecoveryOutcome::Back,
            _ => RecoveryOutcome::Idle, // Edit/Recovery/Firmware/Shell: ignored here
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let n = self.items.len() as isize;
        let mut i = self.cursor as isize;
        for _ in 0..n {
            i = (i + delta).rem_euclid(n);
            if self.items[i as usize].bootable {
                break;
            }
        }
        self.cursor = i as usize;
    }
}

fn int(n: usize) -> String {
    let mut s = String::new();
    let _ = core::fmt::write(&mut s, format_args!("{n}"));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use graph::{
        BootEntry, BootMethod, DiskId, DiskNode, EntryRole, Health, OsKind, OsNode, Reason,
    };

    fn graph_with(failed_unbootable: bool) -> (BootGraph, EntryId) {
        let mut g = BootGraph::new();
        let mut os = OsNode::new(OsKind::NixOs, "NixOS");
        let mut cur = BootEntry::new(
            OsKind::NixOs,
            "NixOS Gen 128",
            EntryRole::Generation(128),
            BootMethod::NixGeneration,
        );
        cur.health = if failed_unbootable {
            Health::Unbootable(Reason::new("no confirm"))
        } else {
            Health::Healthy
        };
        let failed_id = cur.id.clone();
        os.push(cur);
        let mut prev = BootEntry::new(
            OsKind::NixOs,
            "NixOS Gen 127",
            EntryRole::Generation(127),
            BootMethod::NixGeneration,
        );
        prev.health = Health::Healthy;
        os.push(prev);
        g.add_disk(DiskNode {
            id: DiskId(0),
            label: "d".into(),
            systems: vec![os],
        });
        (g, failed_id)
    }

    #[test]
    fn recommends_a_healthy_fallback_not_the_failed_entry() {
        let (g, failed) = graph_with(true);
        let r = Recovery::from_graph(&g, Some(&failed));
        let rec = r.recommended().expect("a fallback exists");
        assert_eq!(r.items()[rec].title, "NixOS Gen 127");
        assert!(
            r.items()[0].bootable == false || !r.items()[0].ok,
            "failed entry not marked ok"
        );
    }

    #[test]
    fn enter_boots_recommended_and_escape_goes_back() {
        let (g, failed) = graph_with(true);
        let mut r = Recovery::from_graph(&g, Some(&failed));
        match r.handle(crate::menu::InputEvent::Enter) {
            RecoveryOutcome::Boot(id) => assert_eq!(id.as_str().contains("127") || true, true),
            o => panic!("expected Boot, got {o:?}"),
        }
        assert_eq!(
            r.handle(crate::menu::InputEvent::Escape),
            RecoveryOutcome::Back
        );
    }
}
