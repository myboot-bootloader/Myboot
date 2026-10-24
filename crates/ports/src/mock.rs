//! In-memory mock implementations of the ports for host testing (report §5.4).
//! Kept separate from the trait contracts: these are test doubles, a different
//! responsibility from the port definitions themselves.
extern crate alloc;
use crate::error::{IoError, IoResult, VarNamespace};
use crate::meta::DirEntryMeta;
use crate::traits::{FileStore, VarStore};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Default)]
pub struct MockVarStore {
    global: BTreeMap<String, Vec<u8>>,
    vendor: BTreeMap<String, Vec<u8>>,
}
impl MockVarStore {
    pub fn new() -> Self {
        Self::default()
    }
    fn map(&self, ns: VarNamespace) -> &BTreeMap<String, Vec<u8>> {
        match ns {
            VarNamespace::Global => &self.global,
            VarNamespace::Vendor => &self.vendor,
        }
    }
    fn map_mut(&mut self, ns: VarNamespace) -> &mut BTreeMap<String, Vec<u8>> {
        match ns {
            VarNamespace::Global => &mut self.global,
            VarNamespace::Vendor => &mut self.vendor,
        }
    }
}
impl VarStore for MockVarStore {
    fn get(&self, ns: VarNamespace, name: &str) -> IoResult<Vec<u8>> {
        self.map(ns).get(name).cloned().ok_or(IoError::NotFound)
    }
    fn set(&mut self, ns: VarNamespace, name: &str, value: &[u8]) -> IoResult<()> {
        self.map_mut(ns).insert(name.to_string(), value.to_vec());
        Ok(())
    }
    fn delete(&mut self, ns: VarNamespace, name: &str) -> IoResult<()> {
        self.map_mut(ns).remove(name);
        Ok(())
    }
    fn list(&self, ns: VarNamespace) -> IoResult<Vec<String>> {
        Ok(self.map(ns).keys().cloned().collect())
    }
}

#[derive(Default)]
pub struct MockFileStore {
    files: BTreeMap<String, Vec<u8>>,
    /// Packed modification times (see `DirEntryMeta::modified`); 0 if unset.
    mtimes: BTreeMap<String, u64>,
}
impl MockFileStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_file(mut self, path: &str, data: &[u8]) -> Self {
        self.files.insert(path.to_string(), data.to_vec());
        self
    }
    pub fn with_mtime(mut self, path: &str, modified: u64) -> Self {
        self.mtimes.insert(path.to_string(), modified);
        self
    }
    /// The directory prefix with exactly one trailing separator (`/` or `\`).
    fn dir_prefix(path: &str) -> (String, char) {
        let sep = if path.contains('\\') { '\\' } else { '/' };
        let mut prefix = String::from(path);
        if !prefix.ends_with(sep) {
            prefix.push(sep);
        }
        (prefix, sep)
    }
}
impl FileStore for MockFileStore {
    fn read(&self, path: &str) -> IoResult<Vec<u8>> {
        self.files.get(path).cloned().ok_or(IoError::NotFound)
    }
    fn write(&mut self, path: &str, data: &[u8]) -> IoResult<()> {
        self.files.insert(path.to_string(), data.to_vec());
        Ok(())
    }
    fn append(&mut self, path: &str, data: &[u8]) -> IoResult<()> {
        self.files
            .entry(path.to_string())
            .or_default()
            .extend_from_slice(data);
        Ok(())
    }
    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }
    fn read_head(&self, path: &str, max_len: usize) -> IoResult<Vec<u8>> {
        let data = self.files.get(path).ok_or(IoError::NotFound)?;
        Ok(data[..data.len().min(max_len)].to_vec())
    }
    fn list_dir(&self, path: &str) -> IoResult<Vec<String>> {
        // Normalise: ensure a single trailing separator, accept `/` or `\`.
        let (prefix, sep) = Self::dir_prefix(path);
        let mut names: Vec<String> = Vec::new();
        for full in self.files.keys() {
            if let Some(rest) = full.strip_prefix(&prefix) {
                let child = rest.split(sep).next().unwrap_or(rest);
                if !child.is_empty() && !names.iter().any(|n| n == child) {
                    names.push(child.to_string());
                }
            }
        }
        if names.is_empty() && !self.files.keys().any(|k| k.starts_with(&prefix)) {
            return Err(IoError::NotFound);
        }
        Ok(names)
    }
    fn list_dir_meta(&self, path: &str) -> IoResult<Vec<DirEntryMeta>> {
        // Same children as `list_dir`. A child is a file if `prefix + child` is a
        // stored path; otherwise it is an implicit directory (size 0).
        let (prefix, _) = Self::dir_prefix(path);
        let mut out = Vec::new();
        for name in self.list_dir(path)? {
            let full = alloc::format!("{prefix}{name}");
            let (size, is_dir) = match self.files.get(&full) {
                Some(data) => (data.len() as u64, false),
                None => (0, true),
            };
            let modified = self.mtimes.get(&full).copied().unwrap_or(0);
            out.push(DirEntryMeta {
                name,
                size,
                modified,
                is_dir,
            });
        }
        Ok(out)
    }
}
