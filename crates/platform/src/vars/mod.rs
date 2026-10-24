//! `VarStore` implemented over UEFI variable runtime services (report §3.9,
//! verified facts). Maps MyBoot's namespaces to the EFI global GUID and MyBoot's
//! vendor GUID, and firmware `Status` codes to the port's `IoError`.
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use ports::{IoError, IoResult, VarNamespace, VarStore};
use uefi::runtime::{self, VariableAttributes, VariableVendor};
use uefi::{guid, Status};

/// EFI global-variable GUID (Boot####, BootOrder, BootNext, BootCurrent).
const GLOBAL: VariableVendor = VariableVendor(guid!("8be4df61-93ca-11d2-aa0d-00e098032b8c"));
/// MyBoot's private vendor GUID for its own state.
const VENDOR: VariableVendor = VariableVendor(guid!("6f4e0b2a-1c3d-4e5f-9a8b-0c1d2e3f4a5b"));

/// Zero-sized adapter: all state lives in firmware. One responsibility — move
/// bytes between the port and the variable services (SRP).
pub struct UefiVarStore;

impl UefiVarStore {
    pub fn new() -> Self {
        UefiVarStore
    }
    fn vendor(ns: VarNamespace) -> &'static VariableVendor {
        match ns {
            VarNamespace::Global => &GLOBAL,
            VarNamespace::Vendor => &VENDOR,
        }
    }
}
impl Default for UefiVarStore {
    fn default() -> Self {
        Self::new()
    }
}

fn map_err(s: Status) -> IoError {
    match s {
        Status::NOT_FOUND => IoError::NotFound,
        Status::ACCESS_DENIED | Status::WRITE_PROTECTED | Status::SECURITY_VIOLATION => {
            IoError::AccessDenied
        }
        Status::BUFFER_TOO_SMALL | Status::OUT_OF_RESOURCES => IoError::TooLarge,
        Status::DEVICE_ERROR => IoError::DeviceError,
        other => IoError::Other(alloc::format!("uefi status {other:?}")),
    }
}

fn to_cstr16(name: &str) -> Result<uefi::CString16, IoError> {
    uefi::CString16::try_from(name).map_err(|_| IoError::Other("bad variable name".to_string()))
}

impl VarStore for UefiVarStore {
    fn get(&self, ns: VarNamespace, name: &str) -> IoResult<Vec<u8>> {
        let cname = to_cstr16(name)?;
        let vendor = Self::vendor(ns);
        match runtime::get_variable_boxed(&cname, vendor) {
            Ok((data, _attrs)) => Ok(data.into_vec()),
            Err(e) => Err(map_err(e.status())),
        }
    }

    fn set(&mut self, ns: VarNamespace, name: &str, value: &[u8]) -> IoResult<()> {
        let cname = to_cstr16(name)?;
        let vendor = Self::vendor(ns);
        let attrs = VariableAttributes::NON_VOLATILE
            | VariableAttributes::BOOTSERVICE_ACCESS
            | VariableAttributes::RUNTIME_ACCESS;
        runtime::set_variable(&cname, vendor, attrs, value).map_err(|e| map_err(e.status()))
    }

    fn delete(&mut self, ns: VarNamespace, name: &str) -> IoResult<()> {
        let cname = to_cstr16(name)?;
        let vendor = Self::vendor(ns);
        match runtime::set_variable(&cname, vendor, VariableAttributes::empty(), &[]) {
            Ok(()) => Ok(()),
            Err(e) if e.status() == Status::NOT_FOUND => Ok(()),
            Err(e) => Err(map_err(e.status())),
        }
    }

    fn list(&self, ns: VarNamespace) -> IoResult<Vec<String>> {
        let want = Self::vendor(ns);
        let mut out = Vec::new();
        // uefi 0.33: `variable_keys()` returns the `VariableKeys` iterator
        // directly (not a `Result`), and each item is a `Result<VariableKey>` —
        // an `Err` means a non-UCS-2 variable name, which per the 0.33 changelog
        // does not stop iteration, so we skip it and continue.
        for entry in runtime::variable_keys() {
            let key = match entry {
                Ok(k) => k,
                Err(_) => continue,
            };
            if &key.vendor == want {
                // `name` is a public `CString16` field in 0.33 (the `name()`
                // method is deprecated), and always holds a valid UCS-2 string.
                out.push(key.name.to_string());
            }
        }
        Ok(out)
    }
}
