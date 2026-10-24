//! The platform's Secure Boot posture, as reported by firmware variables
//! `SecureBoot` and `SetupMode` (verified facts). The `platform` adapter reads
//! these; this type is the pure representation the policy reasons over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecureBootState {
    /// Secure Boot is enforcing signatures.
    Enabled,
    /// Secure Boot is off — the firmware will launch anything.
    Disabled,
    /// Setup Mode: keys are being enrolled; treated as not-yet-enforcing.
    SetupMode,
    /// The state could not be determined (variable missing/unreadable).
    Unknown,
}

impl SecureBootState {
    /// Decode from the raw `SecureBoot` (and optional `SetupMode`) variable bytes.
    /// Both are single-byte booleans per the UEFI spec. Total.
    pub fn from_vars(secure_boot: Option<&[u8]>, setup_mode: Option<&[u8]>) -> Self {
        let sb = secure_boot.and_then(|b| b.first().copied());
        let sm = setup_mode.and_then(|b| b.first().copied());
        match (sb, sm) {
            (_, Some(1)) => SecureBootState::SetupMode,
            (Some(1), _) => SecureBootState::Enabled,
            (Some(0), _) => SecureBootState::Disabled,
            _ => SecureBootState::Unknown,
        }
    }

    pub fn is_enforcing(self) -> bool {
        matches!(self, SecureBootState::Enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_firmware_bytes() {
        assert_eq!(
            SecureBootState::from_vars(Some(&[1]), Some(&[0])),
            SecureBootState::Enabled
        );
        assert_eq!(
            SecureBootState::from_vars(Some(&[0]), None),
            SecureBootState::Disabled
        );
        assert_eq!(
            SecureBootState::from_vars(Some(&[1]), Some(&[1])),
            SecureBootState::SetupMode
        );
        assert_eq!(
            SecureBootState::from_vars(None, None),
            SecureBootState::Unknown
        );
        assert!(SecureBootState::from_vars(Some(&[1]), None).is_enforcing());
    }
}
