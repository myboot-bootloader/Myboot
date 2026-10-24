//! Tier-1 state: MyBoot's small must-persist values in the vendor variable
//! namespace (report §3.9). Kept deliberately tiny — NVRAM is scarce and wears.
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use graph::EntryId;
use ports::{IoError, IoResult, VarNamespace, VarStore};

/// Variable names under the vendor GUID. One concept per variable (SRP).
pub const VAR_DEFAULT: &str = "Default"; // stable EntryId of the configured default
pub const VAR_ONESHOT: &str = "OneShot"; // stable EntryId to try once, then clear
pub const VAR_LASTGOOD: &str = "LastGood"; // stable EntryId of the last confirmed boot

/// Read a stored EntryId from a vendor variable using only `&V` (for callers
/// that do not have mutable access). Returns Ok(None) if the variable is absent.
pub fn read_id<V: VarStore>(vars: &V, name: &str) -> IoResult<Option<EntryId>> {
    match vars.get(VarNamespace::Vendor, name) {
        Ok(bytes) => {
            let s = String::from_utf8(bytes).map_err(|_| IoError::Corrupt)?;
            Ok(Some(EntryId::from_raw(s)))
        }
        Err(IoError::NotFound) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Read the configured default entry with only `&V`.
pub fn read_default<V: VarStore>(vars: &V) -> IoResult<Option<EntryId>> {
    read_id(vars, VAR_DEFAULT)
}
/// Read the last confirmed-good entry with only `&V`.
pub fn read_last_good<V: VarStore>(vars: &V) -> IoResult<Option<EntryId>> {
    read_id(vars, VAR_LASTGOOD)
}

/// Thin accessor over a `VarStore`. Holds no state itself (SRP: it only maps
/// MyBoot concepts to vendor variables).
pub struct StateStore<'a, V: VarStore> {
    vars: &'a mut V,
}

impl<'a, V: VarStore> StateStore<'a, V> {
    pub fn new(vars: &'a mut V) -> Self {
        StateStore { vars }
    }

    pub fn default_entry(&self) -> IoResult<Option<EntryId>> {
        self.read_id(VAR_DEFAULT)
    }
    pub fn set_default(&mut self, id: &EntryId) -> IoResult<()> {
        self.write_id(VAR_DEFAULT, id)
    }

    pub fn last_good(&self) -> IoResult<Option<EntryId>> {
        self.read_id(VAR_LASTGOOD)
    }
    pub fn set_last_good(&mut self, id: &EntryId) -> IoResult<()> {
        self.write_id(VAR_LASTGOOD, id)
    }

    /// The one-shot entry is consumed on read (read-and-clear), mirroring how
    /// firmware treats BootNext (report verified facts).
    pub fn take_one_shot(&mut self) -> IoResult<Option<EntryId>> {
        let id = self.read_id(VAR_ONESHOT)?;
        if id.is_some() {
            self.vars.delete(VarNamespace::Vendor, VAR_ONESHOT)?;
        }
        Ok(id)
    }
    pub fn set_one_shot(&mut self, id: &EntryId) -> IoResult<()> {
        self.write_id(VAR_ONESHOT, id)
    }

    fn read_id(&self, name: &str) -> IoResult<Option<EntryId>> {
        read_id(self.vars, name)
    }
    fn write_id(&mut self, name: &str, id: &EntryId) -> IoResult<()> {
        let bytes: Vec<u8> = id.as_str().as_bytes().to_vec();
        self.vars.set(VarNamespace::Vendor, name, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph::{EntryId, EntryRole, OsKind};
    use ports::mock::MockVarStore;

    #[test]
    fn default_and_last_good_persist() {
        let mut vs = MockVarStore::new();
        let id = EntryId::derive(OsKind::NixOs, &EntryRole::Generation(128), None);
        {
            let mut st = StateStore::new(&mut vs);
            assert_eq!(st.default_entry().unwrap(), None);
            st.set_default(&id).unwrap();
            st.set_last_good(&id).unwrap();
        }
        let st = StateStore::new(&mut vs);
        assert_eq!(st.default_entry().unwrap().unwrap(), id);
        assert_eq!(st.last_good().unwrap().unwrap(), id);
    }

    #[test]
    fn one_shot_is_consumed_on_read() {
        let mut vs = MockVarStore::new();
        let id = EntryId::derive(OsKind::Windows, &EntryRole::Default, None);
        let mut st = StateStore::new(&mut vs);
        st.set_one_shot(&id).unwrap();
        assert_eq!(st.take_one_shot().unwrap().unwrap(), id);
        assert_eq!(
            st.take_one_shot().unwrap(),
            None,
            "one-shot must clear after use"
        );
    }
}
