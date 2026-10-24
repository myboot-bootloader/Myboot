//! The policy inputs and outputs (report §2.7, §3.3). Pure data: the strategy
//! selector, the history the decision may consult, and the explainable result.
//! Kept separate from the algorithm so the "what" is readable without the "how".
extern crate alloc;
use alloc::string::String;
use graph::EntryId;

/// How the default is chosen. Mirrors `config::PolicyKind` but is defined here so
/// the domain core does not depend on config (dependency direction stays inward).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// Always prefer the configured default entry (if bootable).
    Default,
    /// Prefer the last confirmed-good entry, else the configured default.
    LastGoodThenDefault,
}

/// Inputs the decision may consult, beyond the graph itself. All optional; the
/// policy degrades gracefully when history is absent (first boot).
#[derive(Clone, Debug, Default)]
pub struct History {
    /// The configured default entry id (from config), if any.
    pub configured_default: Option<EntryId>,
    /// The last entry that confirmed a successful boot, if any.
    pub last_good: Option<EntryId>,
    /// Entries the user asked to try once (one-shot), highest priority.
    pub one_shot: Option<EntryId>,
}

/// A decision plus the human-readable justification (report §2.7 "explainable").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub chosen: EntryId,
    pub reason: Reason,
}

/// Why an entry was chosen — a closed set, so callers can localise/log it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    OneShot,
    ConfiguredDefault,
    LastGood,
    /// The configured/last-good choice was not bootable, so we fell back.
    FellBackToHealthiest {
        skipped: String,
    },
    /// Nothing was configured; picked the single healthiest bootable entry.
    HealthiestAvailable,
}
