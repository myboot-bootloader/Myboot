//! engine — the composition root (report §3.3, Figure 4.1). This is the ONE place
//! that wires the pure domain core to the UEFI adapters and runs the boot
//! pipeline. Everything it orchestrates is defined and tested elsewhere; the
//! engine's job is sequencing and I/O, not policy.
//!
//! The 9-stage pipeline (report §3.3):
//!   1. read persisted state + the pending boot marker
//!   2. discover the world → Boot Graph
//!   3. assess health over the graph
//!   4. reconcile the previous boot (commit-or-rollback via the marker)
//!   5. load + validate config
//!   6. decide the default (deterministic, explainable policy)
//!   7. present the menu (graphical) or shell, honouring the timeout
//!   8. run the boot transaction: validate → stage (charge) → arm marker
//!   9. resolve the launch plan and hand off to firmware (ExitBootServices)
//!
//! Requires the `x86_64-unknown-uefi` target (see BUILD_AND_TROUBLESHOOT.md).
#![no_std]
extern crate alloc;

mod canvas_impl;
mod direct_boot;
mod pipeline;
mod timing;

pub use pipeline::run;
