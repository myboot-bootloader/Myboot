//! The cooperative boot-result marker (report §3.7). MyBoot follows the pattern
//! established by the systemd Boot Loader Interface / `systemd-bless-boot`: the
//! loader records which entry it is trying and how many attempts remain; the
//! booted OS, once healthy, "blesses" the boot as good. On the next boot MyBoot
//! reads the marker to decide commit vs rollback.
//!
//! This module owns ONLY the encode/decode of that marker in the vendor variable
//! namespace (SRP). The engine wires it to the transaction; the OS-side blessing
//! is done by the host CLI (`myboot bless`) or a small OS unit.
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use graph::EntryId;
use ports::{IoError, IoResult, VarNamespace, VarStore};

/// Vendor variable holding the in-flight boot marker.
pub const VAR_BOOT_ATTEMPT: &str = "BootAttempt";

/// The marker persisted across the handoff: which entry, and whether the OS has
/// confirmed it. Encoded as `"<confirmed>\t<entry-id>"` — tiny and greppable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marker {
    pub entry: EntryId,
    pub confirmed: bool,
}

impl Marker {
    pub fn attempting(entry: EntryId) -> Self {
        Marker {
            entry,
            confirmed: false,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut s = String::new();
        s.push(if self.confirmed { '1' } else { '0' });
        s.push('\t');
        s.push_str(self.entry.as_str());
        s.into_bytes()
    }

    /// Total decode: malformed markers are treated as absent (Err::Corrupt).
    pub fn decode(bytes: &[u8]) -> Result<Marker, IoError> {
        let s = core::str::from_utf8(bytes).map_err(|_| IoError::Corrupt)?;
        let (flag, id) = s.split_once('\t').ok_or(IoError::Corrupt)?;
        Ok(Marker {
            entry: EntryId::from_raw(id),
            confirmed: flag == "1",
        })
    }
}

/// Write the "attempting <entry>" marker before handoff (called at Stage).
pub fn arm<V: VarStore>(vars: &mut V, entry: &EntryId) -> IoResult<()> {
    let m = Marker::attempting(entry.clone());
    vars.set(VarNamespace::Vendor, VAR_BOOT_ATTEMPT, &m.encode())
}

/// Read the marker at the start of the next boot to decide commit vs rollback.
/// Returns None if there is no marker (a clean boot).
pub fn read<V: VarStore>(vars: &V) -> IoResult<Option<Marker>> {
    match vars.get(VarNamespace::Vendor, VAR_BOOT_ATTEMPT) {
        Ok(bytes) => Marker::decode(&bytes).map(Some),
        Err(IoError::NotFound) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Clear the marker after it has been acted upon (commit or rollback complete).
pub fn clear<V: VarStore>(vars: &mut V) -> IoResult<()> {
    vars.delete(VarNamespace::Vendor, VAR_BOOT_ATTEMPT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph::{EntryId, EntryRole, OsKind};
    use ports::mock::MockVarStore;

    fn id() -> EntryId {
        EntryId::derive(OsKind::NixOs, &EntryRole::Generation(128), None)
    }

    #[test]
    fn arm_read_clear_cycle() {
        let mut vs = MockVarStore::new();
        assert_eq!(read(&vs).unwrap(), None);
        arm(&mut vs, &id()).unwrap();
        let m = read(&vs).unwrap().unwrap();
        assert_eq!(m.entry, id());
        assert!(!m.confirmed, "freshly armed marker is unconfirmed");
        clear(&mut vs).unwrap();
        assert_eq!(read(&vs).unwrap(), None);
    }

    #[test]
    fn marker_roundtrips_and_decode_is_total() {
        let m = Marker {
            entry: id(),
            confirmed: true,
        };
        assert_eq!(Marker::decode(&m.encode()).unwrap(), m);
        assert!(Marker::decode(b"no-tab-here").is_err());
        assert!(Marker::decode(&[0xFF, 0xFE]).is_err()); // invalid utf8
    }
}
