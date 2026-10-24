//! policy — the deterministic, explainable default-selection engine
//! (report §2.7, §3.3; blueprint rule 4). Given the Boot Graph and a little
//! history, it decides which entry should be the default — and, crucially, WHY.
//!
//! HARD RULE: no opaque model, no randomness, no "AI" in the boot decision path.
//! Every recommendation carries a machine-readable `Reason`.
//!
//! Split by responsibility: `model` (the strategy/history/decision data) and
//! `decide` (the pure algorithm). Both re-exported for a flat `policy::X` path.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

mod decide;
mod model;

pub use decide::{decide, ranked_candidates};
pub use model::{Decision, History, Reason, Strategy};

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec::Vec;
    use graph::{
        BootEntry, BootGraph, BootMethod, DiskId, DiskNode, EntryId, EntryRole, Health, OsKind,
        OsNode, Reason as GReason,
    };

    fn entry(g: u32, health: Health) -> BootEntry {
        let mut e = BootEntry::new(
            OsKind::NixOs,
            "gen",
            EntryRole::Generation(g),
            BootMethod::NixGeneration,
        );
        e.health = health;
        e
    }
    fn graph_with(entries: Vec<BootEntry>) -> BootGraph {
        let mut g = BootGraph::new();
        let mut os = OsNode::new(OsKind::NixOs, "NixOS");
        for e in entries {
            os.push(e);
        }
        g.add_disk(DiskNode {
            id: DiskId(0),
            label: "d".to_string(),
            systems: alloc::vec![os],
        });
        g
    }
    fn id(g: u32) -> EntryId {
        EntryId::derive(OsKind::NixOs, &EntryRole::Generation(g), None)
    }

    #[test]
    fn one_shot_wins_when_bootable() {
        let g = graph_with(alloc::vec![
            entry(128, Health::Healthy),
            entry(127, Health::Healthy)
        ]);
        let h = History {
            one_shot: Some(id(127)),
            ..Default::default()
        };
        let d = decide(&g, &h, Strategy::LastGoodThenDefault).unwrap();
        assert_eq!(d.chosen, id(127));
        assert_eq!(d.reason, Reason::OneShot);
    }

    #[test]
    fn last_good_preferred_then_falls_back_when_unbootable() {
        let g = graph_with(alloc::vec![
            entry(128, Health::Healthy),
            entry(127, Health::Unbootable(GReason::new("missing kernel"))),
        ]);
        let h = History {
            last_good: Some(id(127)),
            ..Default::default()
        };
        let d = decide(&g, &h, Strategy::LastGoodThenDefault).unwrap();
        assert_eq!(d.chosen, id(128));
        match d.reason {
            Reason::FellBackToHealthiest { skipped } => assert!(skipped.contains("127")),
            other => panic!("expected fallback, got {other:?}"),
        }
    }

    #[test]
    fn healthiest_available_when_no_history() {
        let g = graph_with(alloc::vec![
            entry(126, Health::Degraded(GReason::new("kernel changed"))),
            entry(128, Health::Healthy),
        ]);
        let d = decide(&g, &History::default(), Strategy::LastGoodThenDefault).unwrap();
        assert_eq!(d.chosen, id(128));
        assert_eq!(d.reason, Reason::HealthiestAvailable);
    }

    #[test]
    fn none_when_nothing_bootable() {
        let g = graph_with(alloc::vec![entry(
            128,
            Health::Unbootable(GReason::new("x"))
        )]);
        assert!(decide(&g, &History::default(), Strategy::LastGoodThenDefault).is_none());
    }

    #[test]
    fn decision_is_deterministic() {
        let g = graph_with(alloc::vec![
            entry(128, Health::Healthy),
            entry(127, Health::Healthy)
        ]);
        let a = decide(&g, &History::default(), Strategy::LastGoodThenDefault);
        let b = decide(&g, &History::default(), Strategy::LastGoodThenDefault);
        assert_eq!(a, b);
    }
}
