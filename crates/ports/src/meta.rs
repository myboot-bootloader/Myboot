//! Directory-entry metadata returned by `FileStore::list_dir_meta`. One job:
//! describe a directory entry without its contents.
extern crate alloc;
use alloc::string::String;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntryMeta {
    pub name: String,
    pub size: u64,
    /// Packed modification time:
    /// `year<<40 | month<<32 | day<<24 | hour<<16 | minute<<8 | second`.
    pub modified: u64,
    pub is_dir: bool,
}
