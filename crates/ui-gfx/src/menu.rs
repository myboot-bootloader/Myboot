//! The pure menu model (report §3.10.1). Holds the flattened, selectable list of
//! entries derived from the Boot Graph, tracks the cursor, and maps input events
//! to outcomes. No drawing, no firmware — fully host-tested.
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use graph::{BootGraph, EntryId, Health};

/// One selectable row: what to show and whether it can be booted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: EntryId,
    pub title: String,
    pub os: String,
    pub health: HealthTag,
    pub bootable: bool,
}

/// A render-friendly health tag (keeps `theme` colour choice out of the model).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthTag {
    Ok,
    Warn,
    Fail,
    Unknown,
}

impl HealthTag {
    pub fn of(h: &Health) -> Self {
        match h {
            Health::Healthy => HealthTag::Ok,
            Health::Degraded(_) => HealthTag::Warn,
            Health::Unbootable(_) => HealthTag::Fail,
            Health::Unknown => HealthTag::Unknown,
        }
    }
}

/// Input events the menu understands (mapped from raw key events by the caller).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Up,
    Down,
    Enter,
    Escape,
    Timeout,
    Edit,
    Recovery,
    Firmware,
    Shell,
}

/// What the engine should do after handling an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuOutcome {
    Idle,
    Boot(EntryId),
    EditCmdline(EntryId),
    Recovery,
    Firmware,
    Shell,
}

/// The menu state: the rows and the current cursor position.
pub struct Menu {
    rows: Vec<Row>,
    cursor: usize,
}

impl Menu {
    /// Build from the Boot Graph in canonical order. The initial cursor is the
    /// first bootable row (so the default action is always safe).
    pub fn from_graph(graph: &BootGraph) -> Self {
        let mut rows = Vec::new();
        for d in &graph.disks {
            for os in &d.systems {
                for e in &os.entries {
                    rows.push(Row {
                        id: e.id.clone(),
                        title: e.title.clone(),
                        os: os.name.clone(),
                        health: HealthTag::of(&e.health),
                        bootable: e.bootable(),
                    });
                }
            }
        }
        let cursor = rows.iter().position(|r| r.bootable).unwrap_or(0);
        Menu { rows, cursor }
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn selected(&self) -> Option<&Row> {
        self.rows.get(self.cursor)
    }

    /// Handle one input event, updating the cursor and possibly returning an
    /// outcome. Navigation skips nothing (you can inspect unbootable entries) but
    /// Enter on an unbootable row does not boot.
    pub fn handle(&mut self, ev: InputEvent) -> MenuOutcome {
        match ev {
            InputEvent::Up => {
                self.move_cursor(-1);
                MenuOutcome::Idle
            }
            InputEvent::Down => {
                self.move_cursor(1);
                MenuOutcome::Idle
            }
            InputEvent::Enter | InputEvent::Timeout => match self.selected() {
                Some(r) if r.bootable => MenuOutcome::Boot(r.id.clone()),
                _ => MenuOutcome::Idle,
            },
            InputEvent::Edit => match self.selected() {
                Some(r) if r.bootable => MenuOutcome::EditCmdline(r.id.clone()),
                _ => MenuOutcome::Idle,
            },
            InputEvent::Recovery => MenuOutcome::Recovery,
            InputEvent::Firmware => MenuOutcome::Firmware,
            InputEvent::Shell => MenuOutcome::Shell,
            // Esc has nothing to go "back" to on the top menu; it's a no-op there.
            InputEvent::Escape => MenuOutcome::Idle,
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let n = self.rows.len() as isize;
        let mut c = self.cursor as isize + delta;
        if c < 0 {
            c = n - 1;
        } // wrap around
        if c >= n {
            c = 0;
        }
        self.cursor = c as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use graph::{
        BootEntry, BootGraph, BootMethod, DiskId, DiskNode, EntryRole, Health, OsKind, OsNode,
        Reason,
    };

    fn graph() -> BootGraph {
        let mut g = BootGraph::new();
        let mut os = OsNode::new(OsKind::NixOs, "NixOS");
        let mut a = BootEntry::new(
            OsKind::NixOs,
            "gen128",
            EntryRole::Generation(128),
            BootMethod::NixGeneration,
        );
        a.health = Health::Healthy;
        let mut b = BootEntry::new(
            OsKind::NixOs,
            "gen127",
            EntryRole::Generation(127),
            BootMethod::NixGeneration,
        );
        b.health = Health::Unbootable(Reason::new("x"));
        os.push(a);
        os.push(b);
        g.add_disk(DiskNode {
            id: DiskId(0),
            label: "d".to_string(),
            systems: alloc::vec![os],
        });
        g.sort_canonical();
        g
    }

    #[test]
    fn initial_cursor_is_first_bootable() {
        let m = Menu::from_graph(&graph());
        assert_eq!(m.selected().unwrap().title, "gen128");
        assert!(m.selected().unwrap().bootable);
    }

    #[test]
    fn navigation_wraps_and_tracks_cursor() {
        let mut m = Menu::from_graph(&graph());
        assert_eq!(m.cursor(), 0);
        m.handle(InputEvent::Down);
        assert_eq!(m.cursor(), 1);
        m.handle(InputEvent::Down); // wraps back to 0
        assert_eq!(m.cursor(), 0);
        m.handle(InputEvent::Up); // wraps to last
        assert_eq!(m.cursor(), 1);
    }

    #[test]
    fn enter_on_bootable_boots_but_not_on_unbootable() {
        let mut m = Menu::from_graph(&graph());
        assert!(matches!(m.handle(InputEvent::Enter), MenuOutcome::Boot(_)));
        m.handle(InputEvent::Down); // move to unbootable gen127
        assert_eq!(m.handle(InputEvent::Enter), MenuOutcome::Idle);
    }

    #[test]
    fn hotkeys_route_to_the_right_outcomes() {
        let mut m = Menu::from_graph(&graph());
        assert_eq!(m.handle(InputEvent::Recovery), MenuOutcome::Recovery);
        assert_eq!(m.handle(InputEvent::Firmware), MenuOutcome::Firmware);
        assert_eq!(m.handle(InputEvent::Shell), MenuOutcome::Shell);
        assert_eq!(m.handle(InputEvent::Escape), MenuOutcome::Idle); // nothing to go back to
                                                                     // Edit yields EditCmdline for a bootable selection.
        match m.handle(InputEvent::Edit) {
            MenuOutcome::EditCmdline(_) => {}
            o => panic!("expected EditCmdline, got {o:?}"),
        }
    }

    #[test]
    fn timeout_boots_the_selected_default() {
        let mut m = Menu::from_graph(&graph());
        assert!(matches!(
            m.handle(InputEvent::Timeout),
            MenuOutcome::Boot(_)
        ));
    }
}
