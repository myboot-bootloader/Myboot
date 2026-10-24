//! The decision algorithm (report §2.7, §3.3; blueprint rule 4). A pure,
//! deterministic fold over the Boot Graph — no randomness, no opaque model, no
//! "AI" in the boot path. Every recommendation carries a machine-readable reason.
extern crate alloc;
use crate::model::{Decision, History, Reason, Strategy};
use alloc::vec::Vec;
use graph::{BootEntry, BootGraph, EntryId, Health};

/// Decide the default entry. Deterministic and total: returns `None` only when
/// the graph has no bootable entry at all (the recovery path handles that).
pub fn decide(graph: &BootGraph, history: &History, strategy: Strategy) -> Option<Decision> {
    // 1. One-shot always wins, if it is bootable.
    if let Some(id) = &history.one_shot {
        if is_bootable(graph, id) {
            return Some(Decision {
                chosen: id.clone(),
                reason: Reason::OneShot,
            });
        }
    }

    // 2. Strategy-preferred choice, if bootable.
    let (preferred, preferred_reason) = match strategy {
        Strategy::LastGoodThenDefault => (
            history
                .last_good
                .clone()
                .or_else(|| history.configured_default.clone()),
            if history.last_good.is_some() {
                Reason::LastGood
            } else {
                Reason::ConfiguredDefault
            },
        ),
        Strategy::Default => (
            history.configured_default.clone(),
            Reason::ConfiguredDefault,
        ),
    };

    if let Some(id) = &preferred {
        if is_bootable(graph, id) {
            return Some(Decision {
                chosen: id.clone(),
                reason: preferred_reason,
            });
        }
    }

    // 3. Fall back to the single healthiest bootable entry, deterministically.
    let healthiest = pick_healthiest(graph)?;
    let reason = match &preferred {
        Some(skipped_id) => Reason::FellBackToHealthiest {
            skipped: skipped_id.as_str().into(),
        },
        None => Reason::HealthiestAvailable,
    };
    Some(Decision {
        chosen: healthiest.id.clone(),
        reason,
    })
}

/// The ordered list of candidates the UI should present as fallbacks, best first.
/// Deterministic: healthiest, then by canonical graph order.
pub fn ranked_candidates(graph: &BootGraph) -> Vec<EntryId> {
    let mut v: Vec<&BootEntry> = graph.bootable().collect();
    v.sort_by(|a, b| b.health.rank().cmp(&a.health.rank()));
    v.into_iter().map(|e| e.id.clone()).collect()
}

fn is_bootable(graph: &BootGraph, id: &EntryId) -> bool {
    graph.find(id).map(|e| e.bootable()).unwrap_or(false)
}

/// Pick the healthiest bootable entry. Ties are broken by canonical graph order
/// (first wins), so the result is deterministic for a given graph.
fn pick_healthiest(graph: &BootGraph) -> Option<&BootEntry> {
    let mut best: Option<&BootEntry> = None;
    for e in graph.bootable() {
        match best {
            None => best = Some(e),
            Some(b) if health_rank(&e.health) > health_rank(&b.health) => best = Some(e),
            _ => {}
        }
    }
    best
}

fn health_rank(h: &Health) -> u8 {
    h.rank()
}
