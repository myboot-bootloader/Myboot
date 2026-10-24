//! Read the platform Secure Boot posture from firmware variables and hand the
//! pure `security::SecureBootState` to the policy layer (report §3.13).
extern crate alloc;
use ports::{VarNamespace, VarStore};

/// Query `SecureBoot` and `SetupMode` (EFI global namespace) and classify.
/// Returns the raw bytes so the caller can build a `security::SecureBootState`
/// (kept as a byte pair here so `platform` need not depend on `security`).
pub fn read_secure_boot<V: VarStore>(
    vars: &V,
) -> (Option<alloc::vec::Vec<u8>>, Option<alloc::vec::Vec<u8>>) {
    let sb = vars.get(VarNamespace::Global, "SecureBoot").ok();
    let sm = vars.get(VarNamespace::Global, "SetupMode").ok();
    (sb, sm)
}
