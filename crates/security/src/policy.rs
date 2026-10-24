//! The image-launch security policy (report §3.13). Given HOW an entry boots and
//! the Secure Boot posture, decide whether to defer to firmware verification,
//! require our own signature check, or refuse.
extern crate alloc;
use crate::state::SecureBootState;
use graph::{BootEntry, BootMethod};

/// The result of our own signature verification of a directly-loaded image.
/// (Chainloaded images are verified by firmware, not here.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verification {
    Valid,
    Invalid,
    NotChecked,
}

/// What the engine should do to launch an entry, security-wise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchDecision {
    /// Load via firmware `LoadImage`/`StartImage`; the firmware enforces Secure
    /// Boot. The safe default for chainloaded and EFI-stub entries.
    DeferToFirmware,
    /// We load the image ourselves (direct kernel) and MUST verify its signature
    /// first because Secure Boot is enforcing.
    RequireOwnVerification,
    /// We load the image ourselves and Secure Boot is not enforcing, so a
    /// signature check is advisory only.
    OwnLoadUnverified,
    /// Refuse to launch, with a reason (e.g. our own verification failed).
    Refuse(&'static str),
}

/// Decide how to launch `entry` under the given Secure Boot `state`, taking into
/// account any verification we have already performed on a self-loaded image.
pub fn decide_launch(
    entry: &BootEntry,
    state: SecureBootState,
    verified: Verification,
) -> LaunchDecision {
    match entry.method {
        // Firmware-launched paths: let the firmware's trust chain decide.
        BootMethod::WindowsBootManager
        | BootMethod::LinuxEfiStub
        | BootMethod::NixGeneration
        | BootMethod::EfiChainload => LaunchDecision::DeferToFirmware,

        // Direct kernel load: WE are responsible for the trust decision.
        BootMethod::LinuxDirect => match (state.is_enforcing(), verified) {
            (true, Verification::Valid) => LaunchDecision::RequireOwnVerification,
            (true, Verification::Invalid) => {
                LaunchDecision::Refuse("signature invalid under Secure Boot")
            }
            (true, Verification::NotChecked) => {
                LaunchDecision::Refuse("unverified image under Secure Boot")
            }
            (false, _) => LaunchDecision::OwnLoadUnverified,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph::{BootEntry, BootMethod, EntryRole, OsKind};

    fn entry(method: BootMethod) -> BootEntry {
        BootEntry::new(OsKind::Linux, "e", EntryRole::Default, method)
    }

    #[test]
    fn chainload_always_defers_to_firmware() {
        let e = entry(BootMethod::WindowsBootManager);
        assert_eq!(
            decide_launch(&e, SecureBootState::Enabled, Verification::NotChecked),
            LaunchDecision::DeferToFirmware
        );
    }

    #[test]
    fn direct_load_requires_valid_signature_when_enforcing() {
        let e = entry(BootMethod::LinuxDirect);
        assert_eq!(
            decide_launch(&e, SecureBootState::Enabled, Verification::Valid),
            LaunchDecision::RequireOwnVerification
        );
        assert!(matches!(
            decide_launch(&e, SecureBootState::Enabled, Verification::Invalid),
            LaunchDecision::Refuse(_)
        ));
        assert!(matches!(
            decide_launch(&e, SecureBootState::Enabled, Verification::NotChecked),
            LaunchDecision::Refuse(_)
        ));
    }

    #[test]
    fn direct_load_unverified_allowed_when_not_enforcing() {
        let e = entry(BootMethod::LinuxDirect);
        assert_eq!(
            decide_launch(&e, SecureBootState::Disabled, Verification::NotChecked),
            LaunchDecision::OwnLoadUnverified
        );
    }
}
