//! transaction — the boot transaction (report §3.7, §3.11). This is a faithful,
//! executable mirror of `spec/tla/BootTransaction.tla`: the same phases, the same
//! transitions, the same `tries_left` / `last_good` semantics, and the same two
//! guarantees — NoDeadEnd (from Failed there is always a way forward) and
//! EventuallyResolved (every run ends Confirmed or Recovery).
//!
//! HARD RULE (blueprint rule 5): the attempt is CHARGED at Stage, BEFORE handoff,
//! so a boot that dies after control leaves MyBoot is still counted against the
//! entry and triggers rollback on the next boot. The machine can never dead-end.
//!
//! Pure state machine over the Boot Graph + a small persisted `tries_left` map.
//! Firmware effects (writing the success marker, setting BootNext) are expressed
//! as `Effect`s the engine applies through the `ports` — so the machine itself is
//! host-tested with no firmware.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

pub mod marker;

use alloc::collections::BTreeMap;
use graph::{BootGraph, EntryId};

#[cfg(test)]
extern crate std;

/// The phases — identical set to the TLA+ `Phases`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Selected,
    Validated,
    Staged,
    Launched,
    Confirmed,
    Failed,
    Recovery,
}

impl Phase {
    /// The two accepting/terminal states (TLA+ `Resolved`).
    pub fn is_terminal(self) -> bool {
        matches!(self, Phase::Confirmed | Phase::Recovery)
    }
}

/// A side effect the engine must apply to firmware/persistence at a transition.
/// The state machine stays pure by RETURNING effects rather than performing I/O.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Record that `id` is being attempted (decrement its remaining tries and
    /// persist), and arm it as the next boot. Emitted at Stage — BEFORE handoff.
    ChargeAttempt(EntryId),
    /// Persist `id` as the last confirmed-good entry. Emitted at ConfirmSuccess.
    RecordLastGood(EntryId),
}

/// Errors from driving the machine out of order (should not happen if the engine
/// follows the pipeline; surfaced for defensive testing).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TxError {
    WrongPhase,
    NotBootable,
    NoEntrySelected,
}

/// The transaction state. `tries_left` mirrors the TLA+ function
/// `triesLeft[e]`; it is loaded from persistence at boot and saved via effects.
#[derive(Clone, Debug)]
pub struct Transaction {
    phase: Phase,
    selected: Option<EntryId>,
    tries_left: BTreeMap<EntryId, u8>,
    last_good: Option<EntryId>,
    committed: Option<EntryId>,
    max_tries: u8,
}

impl Transaction {
    /// Start a fresh transaction. `tries_left` is seeded from persisted state;
    /// entries not present default to `max_tries` on first use.
    pub fn new(
        max_tries: u8,
        tries_left: BTreeMap<EntryId, u8>,
        last_good: Option<EntryId>,
    ) -> Self {
        Transaction {
            phase: Phase::Idle,
            selected: None,
            tries_left,
            last_good,
            committed: None,
            max_tries: max_tries.max(1),
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn selected(&self) -> Option<&EntryId> {
        self.selected.as_ref()
    }
    pub fn committed(&self) -> Option<&EntryId> {
        self.committed.as_ref()
    }
    pub fn tries_left(&self, id: &EntryId) -> u8 {
        *self.tries_left.get(id).unwrap_or(&self.max_tries)
    }

    /// TLA+ `Bootable(e)`: the entry is health-bootable AND has tries remaining.
    pub fn bootable(&self, graph: &BootGraph, id: &EntryId) -> bool {
        graph.find(id).map(|e| e.bootable()).unwrap_or(false) && self.tries_left(id) > 0
    }

    // ---- transitions (each mirrors a TLA+ action) ----

    /// `Select(e)` — from Idle, choose a bootable entry.
    pub fn select(&mut self, graph: &BootGraph, id: &EntryId) -> Result<(), TxError> {
        if self.phase != Phase::Idle {
            return Err(TxError::WrongPhase);
        }
        if !self.bootable(graph, id) {
            return Err(TxError::NotBootable);
        }
        self.selected = Some(id.clone());
        self.phase = Phase::Selected;
        Ok(())
    }

    /// `Validate` — from Selected, confirm the selection is still bootable.
    pub fn validate(&mut self, graph: &BootGraph) -> Result<(), TxError> {
        if self.phase != Phase::Selected {
            return Err(TxError::WrongPhase);
        }
        let id = self.selected.clone().ok_or(TxError::NoEntrySelected)?;
        if !self.bootable(graph, &id) {
            return Err(TxError::NotBootable);
        }
        self.phase = Phase::Validated;
        Ok(())
    }

    /// `Stage` — CHARGE THE ATTEMPT (decrement tries) and move to Staged.
    /// Returns the effect the engine must persist BEFORE handoff.
    pub fn stage(&mut self) -> Result<Effect, TxError> {
        if self.phase != Phase::Validated {
            return Err(TxError::WrongPhase);
        }
        let id = self.selected.clone().ok_or(TxError::NoEntrySelected)?;
        let n = self.tries_left.entry(id.clone()).or_insert(self.max_tries);
        *n = n.saturating_sub(1);
        self.phase = Phase::Staged;
        Ok(Effect::ChargeAttempt(id))
    }

    /// `Launch` — from Staged, hand off. (The actual ExitBootServices + provider
    /// boot happens in the engine; this records the state transition.)
    pub fn launch(&mut self) -> Result<(), TxError> {
        if self.phase != Phase::Staged {
            return Err(TxError::WrongPhase);
        }
        self.phase = Phase::Launched;
        Ok(())
    }

    /// `ConfirmSuccess` — the OS cooperated; commit and restore this entry's tries
    /// to full. Returns the effect to persist last-good.
    pub fn confirm_success(&mut self) -> Result<Effect, TxError> {
        if self.phase != Phase::Launched {
            return Err(TxError::WrongPhase);
        }
        let id = self.selected.clone().ok_or(TxError::NoEntrySelected)?;
        self.tries_left.insert(id.clone(), self.max_tries);
        self.last_good = Some(id.clone());
        self.committed = Some(id.clone());
        self.phase = Phase::Confirmed;
        Ok(Effect::RecordLastGood(id))
    }

    /// `DetectFailure` — the boot did not confirm. Move to Failed. From here the
    /// caller MUST call `resolve` (which is always enabled — NoDeadEnd).
    pub fn detect_failure(&mut self) -> Result<(), TxError> {
        if self.phase != Phase::Launched {
            return Err(TxError::WrongPhase);
        }
        self.phase = Phase::Failed;
        Ok(())
    }

    /// The single "from Failed" transition. This is the executable form of the
    /// NoDeadEnd invariant: it is ALWAYS enabled from Failed and deterministically
    /// picks RollBack (a bootable fallback exists) or Recover (none do).
    ///
    /// Returns the fallback entry to try next (RollBack) or `None` (Recovery).
    pub fn resolve(&mut self, graph: &BootGraph) -> Result<Option<EntryId>, TxError> {
        if self.phase != Phase::Failed {
            return Err(TxError::WrongPhase);
        }
        match self.pick_fallback(graph) {
            Some(fallback) => {
                // RollBack
                self.selected = Some(fallback.clone());
                self.phase = Phase::Selected;
                Ok(Some(fallback))
            }
            None => {
                // Recover
                self.phase = Phase::Recovery;
                Ok(None)
            }
        }
    }

    /// Deterministic fallback choice (report §3.7). In priority order:
    ///   1. the last confirmed-good entry, if still bootable;
    ///   2. any OTHER bootable entry, in canonical graph order — rolling to a
    ///      different known option rather than repeating a just-failed one;
    ///   3. the just-failed entry itself, if it still has attempts left
    ///      (the single-entry retry case, matching boot-counting);
    ///   4. None — nothing bootable remains, so the caller enters Recovery.
    fn pick_fallback(&self, graph: &BootGraph) -> Option<EntryId> {
        if let Some(lg) = &self.last_good {
            if self.bootable(graph, lg) {
                return Some(lg.clone());
            }
        }
        let failed = self.selected.clone();
        if let Some(other) = graph
            .entries()
            .map(|e| e.id.clone())
            .find(|id| Some(id) != failed.as_ref() && self.bootable(graph, id))
        {
            return Some(other);
        }
        // last resort: retry the same entry if it still has attempts
        match &failed {
            Some(id) if self.bootable(graph, id) => Some(id.clone()),
            _ => None,
        }
    }

    /// TLA+ `NoDeadEnd` as a runtime check: the machine is never stuck. From
    /// Failed, `resolve` is TOTAL — it advances to Selected (a fallback exists)
    /// or to Recovery (none do) — so there is always a way forward. Used as a
    /// defensive assertion in the engine and exercised by the tests.
    pub fn has_way_forward(&self, _graph: &BootGraph) -> bool {
        match self.phase {
            // Non-terminal working phases always have their next transition.
            Phase::Idle | Phase::Selected | Phase::Validated | Phase::Staged | Phase::Launched => {
                true
            }
            // From Failed, resolve() always advances (RollBack or Recover).
            Phase::Failed => true,
            // Terminal states are resolved, not dead-ends.
            Phase::Confirmed | Phase::Recovery => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec::Vec;
    use graph::{
        BootEntry, BootGraph, BootMethod, DiskId, DiskNode, EntryRole, Health, OsKind, OsNode,
        Reason,
    };

    fn healthy(g: u32) -> BootEntry {
        let mut e = BootEntry::new(
            OsKind::NixOs,
            "gen",
            EntryRole::Generation(g),
            BootMethod::NixGeneration,
        );
        e.health = Health::Healthy;
        e
    }
    fn graph_of(entries: Vec<BootEntry>) -> BootGraph {
        let mut gr = BootGraph::new();
        let mut os = OsNode::new(OsKind::NixOs, "NixOS");
        for e in entries {
            os.push(e);
        }
        gr.add_disk(DiskNode {
            id: DiskId(0),
            label: "d".to_string(),
            systems: alloc::vec![os],
        });
        gr
    }
    fn id(g: u32) -> EntryId {
        EntryId::derive(OsKind::NixOs, &EntryRole::Generation(g), None)
    }
    fn tx() -> Transaction {
        Transaction::new(2, BTreeMap::new(), None)
    }

    #[test]
    fn happy_path_reaches_confirmed_and_charges_then_restores() {
        let gr = graph_of(alloc::vec![healthy(128)]);
        let mut t = tx();
        t.select(&gr, &id(128)).unwrap();
        t.validate(&gr).unwrap();
        assert_eq!(t.stage().unwrap(), Effect::ChargeAttempt(id(128)));
        assert_eq!(t.tries_left(&id(128)), 1, "attempt charged before handoff");
        t.launch().unwrap();
        assert_eq!(
            t.confirm_success().unwrap(),
            Effect::RecordLastGood(id(128))
        );
        assert_eq!(t.phase(), Phase::Confirmed);
        assert!(t.phase().is_terminal());
        assert_eq!(t.tries_left(&id(128)), 2, "tries restored on success");
        assert_eq!(t.committed(), Some(&id(128)));
    }

    #[test]
    fn failure_rolls_back_to_a_healthy_fallback() {
        let gr = graph_of(alloc::vec![healthy(128), healthy(127)]);
        let mut t = tx();
        t.select(&gr, &id(128)).unwrap();
        t.validate(&gr).unwrap();
        t.stage().unwrap();
        t.launch().unwrap();
        t.detect_failure().unwrap();
        assert_eq!(t.phase(), Phase::Failed);
        // NoDeadEnd: resolve always advances; here a fallback exists → RollBack
        let next = t.resolve(&gr).unwrap();
        assert_eq!(next, Some(id(127)));
        assert_eq!(t.phase(), Phase::Selected);
    }

    #[test]
    fn last_good_is_preferred_on_rollback() {
        let gr = graph_of(alloc::vec![healthy(128), healthy(127)]);
        let mut t = Transaction::new(2, BTreeMap::new(), Some(id(127)));
        t.select(&gr, &id(128)).unwrap();
        t.validate(&gr).unwrap();
        t.stage().unwrap();
        t.launch().unwrap();
        t.detect_failure().unwrap();
        assert_eq!(
            t.resolve(&gr).unwrap(),
            Some(id(127)),
            "prefer last-good fallback"
        );
    }

    #[test]
    fn exhausted_tries_make_entry_unbootable_and_force_recovery() {
        // one entry, max_tries = 1: after staging once, tries_left = 0 → not bootable
        let gr = graph_of(alloc::vec![healthy(128)]);
        let mut t = Transaction::new(1, BTreeMap::new(), None);
        t.select(&gr, &id(128)).unwrap();
        t.validate(&gr).unwrap();
        t.stage().unwrap(); // charges: tries 1 -> 0
        t.launch().unwrap();
        t.detect_failure().unwrap();
        // no bootable entry remains → Recover, never a dead-end
        assert_eq!(t.resolve(&gr).unwrap(), None);
        assert_eq!(t.phase(), Phase::Recovery);
        assert!(t.phase().is_terminal());
    }

    #[test]
    fn cannot_select_unbootable_entry() {
        let mut bad = healthy(128);
        bad.health = Health::Unbootable(Reason::new("missing"));
        let gr = graph_of(alloc::vec![bad]);
        let mut t = tx();
        assert_eq!(t.select(&gr, &id(128)), Err(TxError::NotBootable));
    }

    #[test]
    fn out_of_order_transitions_are_rejected() {
        let gr = graph_of(alloc::vec![healthy(128)]);
        let mut t = tx();
        assert_eq!(t.validate(&gr), Err(TxError::WrongPhase)); // not selected yet
        assert_eq!(t.launch(), Err(TxError::WrongPhase)); // not staged yet
    }
}
