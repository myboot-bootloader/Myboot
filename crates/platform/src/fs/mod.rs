//! `FileStore` over the UEFI Simple File System protocol (report §4.4). Reads and
//! writes files by absolute EFI path and lists directories (backing discovery's
//! ESP/BLS/bootspec scans).
//!
//! STATELESS design: the firmware wants a `&mut` volume handle to open files, but
//! the port's read methods take `&self`. Each call reopens a fresh root, so the
//! adapter needs no interior mutability and stays simple and correct (SRP). The
//! root-based helpers here are reused by `volume::Volume` (a store bound to a
//! specific filesystem handle) so multi-volume discovery shares one code path.
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use ports::{DirEntryMeta, FileStore, IoError, IoResult};
use uefi::boot;
use uefi::proto::media::file::{Directory, File, FileAttribute, FileInfo, FileMode, FileType};
use uefi::{CString16, Status};

/// Zero-sized, stateless adapter over the loaded image's ESP volume.
pub struct UefiFileStore;

impl UefiFileStore {
    pub fn new() -> Self {
        UefiFileStore
    }
}
impl Default for UefiFileStore {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn to_cpath(path: &str) -> Result<CString16, IoError> {
    let norm: String = path
        .chars()
        .map(|c| if c == '/' { '\\' } else { c })
        .collect();
    CString16::try_from(norm.as_str()).map_err(|_| IoError::Other("bad path".to_string()))
}

pub(crate) fn map_err(s: Status) -> IoError {
    match s {
        Status::NOT_FOUND => IoError::NotFound,
        Status::ACCESS_DENIED | Status::WRITE_PROTECTED => IoError::AccessDenied,
        Status::OUT_OF_RESOURCES | Status::VOLUME_FULL => IoError::TooLarge,
        Status::DEVICE_ERROR | Status::MEDIA_CHANGED | Status::NO_MEDIA => IoError::DeviceError,
        other => IoError::Other(alloc::format!("uefi status {other:?}")),
    }
}

/// Open the loaded image's ESP volume root (fresh each call — see module note).
fn open_image_root() -> IoResult<Directory> {
    let mut sfs =
        boot::get_image_file_system(boot::image_handle()).map_err(|e| map_err(e.status()))?;
    sfs.open_volume().map_err(|e| map_err(e.status()))
}

// --- root-based helpers, shared by UefiFileStore and volume::Volume ---

pub(crate) fn read_in(root: &mut Directory, path: &str) -> IoResult<Vec<u8>> {
    let cpath = to_cpath(path)?;
    let handle = root
        .open(&cpath, FileMode::Read, FileAttribute::empty())
        .map_err(|e| map_err(e.status()))?;
    match handle.into_type().map_err(|e| map_err(e.status()))? {
        FileType::Regular(mut f) => {
            let info = f
                .get_boxed_info::<FileInfo>()
                .map_err(|e| map_err(e.status()))?;
            let size = info.file_size() as usize;
            let mut data = alloc::vec![0u8; size];
            let n = f.read(&mut data).map_err(|e| map_err(e.status()))?;
            data.truncate(n);
            Ok(data)
        }
        FileType::Dir(_) => Err(IoError::Other("path is a directory".to_string())),
    }
}

/// Read at most `max_len` bytes from the start of a file (a bounded
/// `RegularFile::read` loop; uefi 0.33 `read` may return fewer bytes than asked).
pub(crate) fn read_head_in(root: &mut Directory, path: &str, max_len: usize) -> IoResult<Vec<u8>> {
    let cpath = to_cpath(path)?;
    let handle = root
        .open(&cpath, FileMode::Read, FileAttribute::empty())
        .map_err(|e| map_err(e.status()))?;
    match handle.into_type().map_err(|e| map_err(e.status()))? {
        FileType::Regular(mut f) => {
            let mut data = alloc::vec![0u8; max_len];
            let mut filled = 0;
            while filled < max_len {
                let n = f
                    .read(&mut data[filled..])
                    .map_err(|e| map_err(e.status()))?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            data.truncate(filled);
            Ok(data)
        }
        FileType::Dir(_) => Err(IoError::Other("path is a directory".to_string())),
    }
}

pub(crate) fn write_in(
    root: &mut Directory,
    path: &str,
    data: &[u8],
    append: bool,
) -> IoResult<()> {
    let cpath = to_cpath(path)?;
    if !append {
        // CreateReadWrite opens an existing file WITHOUT truncating it, so a
        // shorter write would leave the old tail behind. Delete an existing
        // regular file first, then create it fresh. uefi 0.33: `File::delete(self)`
        // (EFI_FILE_PROTOCOL.Delete, which also closes the handle).
        if let Ok(existing) = root.open(&cpath, FileMode::ReadWrite, FileAttribute::empty()) {
            match existing.into_type().map_err(|e| map_err(e.status()))? {
                FileType::Regular(f) => f.delete().map_err(|e| map_err(e.status()))?,
                FileType::Dir(_) => return Err(IoError::Other("path is a directory".to_string())),
            }
        }
    }
    let handle = root
        .open(&cpath, FileMode::CreateReadWrite, FileAttribute::empty())
        .map_err(|e| map_err(e.status()))?;
    match handle.into_type().map_err(|e| map_err(e.status()))? {
        FileType::Regular(mut f) => {
            if append {
                let info = f
                    .get_boxed_info::<FileInfo>()
                    .map_err(|e| map_err(e.status()))?;
                f.set_position(info.file_size())
                    .map_err(|e| map_err(e.status()))?;
            }
            f.write(data).map_err(|_| IoError::DeviceError)?;
            f.flush().map_err(|e| map_err(e.status()))?;
            Ok(())
        }
        FileType::Dir(_) => Err(IoError::Other("path is a directory".to_string())),
    }
}

pub(crate) fn exists_in(root: &mut Directory, path: &str) -> bool {
    match to_cpath(path) {
        Ok(cpath) => root
            .open(&cpath, FileMode::Read, FileAttribute::empty())
            .is_ok(),
        _ => false,
    }
}

/// List a directory with each entry's metadata. uefi 0.33: `FileInfo` exposes
/// `file_size()`, `modification_time() -> &Time` (`year()`..`second()`),
/// `is_directory()` and `file_name()`.
pub(crate) fn list_dir_meta_in(root: &mut Directory, path: &str) -> IoResult<Vec<DirEntryMeta>> {
    let cpath = to_cpath(path)?;
    let handle = root
        .open(&cpath, FileMode::Read, FileAttribute::empty())
        .map_err(|e| map_err(e.status()))?;
    let mut dir = match handle.into_type().map_err(|e| map_err(e.status()))? {
        FileType::Dir(d) => d,
        FileType::Regular(_) => return Err(IoError::Other("path is a file".to_string())),
    };
    let mut out = Vec::new();
    let mut buf = alloc::vec![0u8; 512];
    loop {
        match dir.read_entry(&mut buf) {
            Ok(Some(info)) => {
                let name = info.file_name().to_string();
                if name == "." || name == ".." {
                    continue;
                }
                let t = info.modification_time();
                let modified = ((t.year() as u64) << 40)
                    | ((t.month() as u64) << 32)
                    | ((t.day() as u64) << 24)
                    | ((t.hour() as u64) << 16)
                    | ((t.minute() as u64) << 8)
                    | (t.second() as u64);
                out.push(DirEntryMeta {
                    name,
                    size: info.file_size(),
                    modified,
                    is_dir: info.is_directory(),
                });
            }
            Ok(None) => break,
            Err(e) => {
                if e.status() == Status::BUFFER_TOO_SMALL {
                    buf.resize(buf.len() * 2, 0);
                    continue;
                }
                return Err(map_err(e.status()));
            }
        }
    }
    Ok(out)
}

pub(crate) fn list_dir_in(root: &mut Directory, path: &str) -> IoResult<Vec<String>> {
    Ok(list_dir_meta_in(root, path)?
        .into_iter()
        .map(|m| m.name)
        .collect())
}

impl FileStore for UefiFileStore {
    fn read(&self, path: &str) -> IoResult<Vec<u8>> {
        read_in(&mut open_image_root()?, path)
    }
    fn write(&mut self, path: &str, data: &[u8]) -> IoResult<()> {
        write_in(&mut open_image_root()?, path, data, false)
    }
    fn append(&mut self, path: &str, data: &[u8]) -> IoResult<()> {
        write_in(&mut open_image_root()?, path, data, true)
    }
    fn exists(&self, path: &str) -> bool {
        match open_image_root() {
            Ok(mut r) => exists_in(&mut r, path),
            Err(_) => false,
        }
    }
    fn list_dir(&self, path: &str) -> IoResult<Vec<String>> {
        list_dir_in(&mut open_image_root()?, path)
    }
    fn read_head(&self, path: &str, max_len: usize) -> IoResult<Vec<u8>> {
        read_head_in(&mut open_image_root()?, path, max_len)
    }
    fn list_dir_meta(&self, path: &str) -> IoResult<Vec<DirEntryMeta>> {
        list_dir_meta_in(&mut open_image_root()?, path)
    }
}
