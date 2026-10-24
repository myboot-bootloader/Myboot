//! The driven-port (outbound) trait contracts the domain core owns (ADR 0003;
//! report §3.4, §3.9). The `platform` adapter implements these over uefi-rs;
//! `mock` implements them in memory for host tests. Nothing here knows uefi.
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use crate::error::{VarNamespace, IoResult};
use crate::meta::DirEntryMeta;

/// Read/write firmware variables (report §3.7, §3.9). Implemented by `platform`
/// over `GetVariable`/`SetVariable`; mocked in tests.
pub trait VarStore {
    fn get(&self, ns: VarNamespace, name: &str) -> IoResult<Vec<u8>>;
    fn set(&mut self, ns: VarNamespace, name: &str, value: &[u8]) -> IoResult<()>;
    /// Delete a variable. Deleting an absent variable is Ok (idempotent).
    fn delete(&mut self, ns: VarNamespace, name: &str) -> IoResult<()>;
    fn list(&self, ns: VarNamespace) -> IoResult<Vec<String>>;
}

/// Read/write files on the EFI System Partition (report §3.9). Implemented by
/// `platform` over the Simple File System protocol; mocked in tests.
pub trait FileStore {
    fn read(&self, path: &str) -> IoResult<Vec<u8>>;
    /// Read at most `max_len` bytes from the start of a file. Used where only a
    /// header is needed, so whole kernels/loaders are never loaded to inspect it.
    fn read_head(&self, path: &str, max_len: usize) -> IoResult<Vec<u8>>;
    fn write(&mut self, path: &str, data: &[u8]) -> IoResult<()>;
    /// Append to a file, creating it if absent (used by the boot log).
    fn append(&mut self, path: &str, data: &[u8]) -> IoResult<()>;
    fn exists(&self, path: &str) -> bool;
    /// List the immediate child names of a directory (files and subdirectories),
    /// without their parent path. Used by discovery to scan `\loader\entries`
    /// and the NixOS bootspec directory. `NotFound` if the directory is absent.
    fn list_dir(&self, path: &str) -> IoResult<Vec<String>>;
    /// Like `list_dir`, with each entry's size, modification time and whether it
    /// is a directory. `NotFound` if the directory is absent.
    fn list_dir_meta(&self, path: &str) -> IoResult<Vec<DirEntryMeta>>;
    /// The filesystem's volume label, if any (e.g. an ISO's EFI-partition label).
    /// Default `None`; the UEFI adapter overrides it. Used by discovery as one
    /// source for a human-friendly OS name.
    fn volume_label(&self) -> Option<String> { None }
}
