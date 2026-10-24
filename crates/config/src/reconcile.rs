//! reconcile — bring the firmware's actual boot state into line with the
//! declarative desired state, reversibly (report §3.8, Figure 3.4).
//!
//! The algorithm is: read actual → diff against desired → produce a `Snapshot`
//! (for rollback) plus an ordered `ReconcilePlan` of `PlanStep`s. Planning is
//! PURE (no firmware writes): it reads through the `VarStore` port and returns a
//! plan the engine executes. This keeps the decision testable on the host and
//! keeps a single, auditable place where drift is resolved.
//!
//! Scope note (honest): MyBoot reconciles the boot *order* and the presence of
//! its own boot option. It never rewrites another OS's Boot#### payload — it
//! federates, it does not replace (report §2.7).
extern crate alloc;
use alloc::vec::Vec;
use persistence::loadopt::{decode_boot_order, encode_boot_order};
use ports::{IoError, VarNamespace, VarStore};

pub const VAR_BOOT_ORDER: &str = "BootOrder";

/// A reversible step the engine will apply to firmware.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanStep {
    /// Replace `BootOrder` with this exact sequence of option numbers.
    SetBootOrder(Vec<u16>),
}

/// The plan plus the snapshot needed to undo it (report §3.8: apply → verify →
/// restore on mismatch).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconcilePlan {
    pub steps: Vec<PlanStep>,
    pub snapshot: Snapshot,
}

impl ReconcilePlan {
    /// True when the firmware already matches desired — nothing to do.
    pub fn is_noop(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Everything needed to restore the prior firmware state if verification fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub boot_order: Option<Vec<u16>>,
}

/// Compute the reconciliation plan for a desired boot order.
///
/// `desired` is the option-number sequence MyBoot wants at the head of the boot
/// order (typically: MyBoot's own option first, then the rest, order preserved).
/// Reads actual state through the port; performs NO writes.
pub fn plan_boot_order<V: VarStore>(vars: &V, desired: &[u16]) -> ReconcilePlan {
    let actual = read_boot_order(vars);
    let snapshot = Snapshot {
        boot_order: actual.clone(),
    };

    let matches = match &actual {
        Some(a) => a.as_slice() == desired,
        None => desired.is_empty(),
    };

    let steps = if matches {
        Vec::new()
    } else {
        alloc::vec![PlanStep::SetBootOrder(desired.to_vec())]
    };
    ReconcilePlan { steps, snapshot }
}

/// Desired order that puts `myboot_option` first and preserves the relative
/// order of everything else already present — the drift-repair policy from §3.8
/// (e.g. after a Windows update pushes its own manager to the front).
pub fn desired_with_myboot_first<V: VarStore>(vars: &V, myboot_option: u16) -> Vec<u16> {
    let mut out = Vec::new();
    out.push(myboot_option);
    if let Some(actual) = read_boot_order(vars) {
        for n in actual {
            if n != myboot_option {
                out.push(n);
            }
        }
    }
    out
}

/// Apply a plan through the port. Returns Ok on success; on any write failure the
/// caller should `restore(snapshot)`. (The engine wires apply→verify→restore.)
pub fn apply<V: VarStore>(vars: &mut V, plan: &ReconcilePlan) -> Result<(), IoError> {
    for step in &plan.steps {
        match step {
            PlanStep::SetBootOrder(order) => {
                vars.set(
                    VarNamespace::Global,
                    VAR_BOOT_ORDER,
                    &encode_boot_order(order),
                )?;
            }
        }
    }
    Ok(())
}

/// Restore the pre-reconciliation state from a snapshot (rollback path).
pub fn restore<V: VarStore>(vars: &mut V, snap: &Snapshot) -> Result<(), IoError> {
    match &snap.boot_order {
        Some(order) => vars.set(
            VarNamespace::Global,
            VAR_BOOT_ORDER,
            &encode_boot_order(order),
        ),
        None => vars.delete(VarNamespace::Global, VAR_BOOT_ORDER),
    }
}

fn read_boot_order<V: VarStore>(vars: &V) -> Option<Vec<u16>> {
    match vars.get(VarNamespace::Global, VAR_BOOT_ORDER) {
        Ok(bytes) => Some(decode_boot_order(&bytes)),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ports::mock::MockVarStore;

    fn set_order(vs: &mut MockVarStore, order: &[u16]) {
        vs.set(
            VarNamespace::Global,
            VAR_BOOT_ORDER,
            &encode_boot_order(order),
        )
        .unwrap();
    }
    fn get_order(vs: &MockVarStore) -> Vec<u16> {
        decode_boot_order(&vs.get(VarNamespace::Global, VAR_BOOT_ORDER).unwrap())
    }

    #[test]
    fn noop_when_already_matching() {
        let mut vs = MockVarStore::new();
        set_order(&mut vs, &[0x0000, 0x0001]);
        let plan = plan_boot_order(&vs, &[0x0000, 0x0001]);
        assert!(plan.is_noop());
    }

    #[test]
    fn repairs_drift_and_is_reversible() {
        // Firmware currently boots Windows (1) first; MyBoot is option 0.
        let mut vs = MockVarStore::new();
        set_order(&mut vs, &[0x0001, 0x0000, 0x0002]);

        let desired = desired_with_myboot_first(&vs, 0x0000);
        assert_eq!(desired, alloc::vec![0x0000, 0x0001, 0x0002]);

        let plan = plan_boot_order(&vs, &desired);
        assert!(!plan.is_noop());

        apply(&mut vs, &plan).unwrap();
        assert_eq!(get_order(&vs), alloc::vec![0x0000, 0x0001, 0x0002]);

        // rollback restores the exact prior order
        restore(&mut vs, &plan.snapshot).unwrap();
        assert_eq!(get_order(&vs), alloc::vec![0x0001, 0x0000, 0x0002]);
    }

    #[test]
    fn snapshot_none_when_no_prior_boot_order_and_restore_deletes() {
        let mut vs = MockVarStore::new(); // no BootOrder at all
        let plan = plan_boot_order(&vs, &[0x0000]);
        assert_eq!(plan.snapshot.boot_order, None);
        apply(&mut vs, &plan).unwrap();
        assert_eq!(get_order(&vs), alloc::vec![0x0000]);
        restore(&mut vs, &plan.snapshot).unwrap(); // should delete it again
        assert!(vs.get(VarNamespace::Global, VAR_BOOT_ORDER).is_err());
    }
}
