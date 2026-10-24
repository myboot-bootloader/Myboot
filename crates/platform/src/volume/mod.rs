//! Multi-volume discovery + device-path chainload (report §3.3, §4.5).
//!
//! A real machine has OSes spread across several disks and ESPs. Like rEFInd, we
//! enumerate EVERY filesystem the firmware knows (not just the one we booted
//! from) and scan them all. `Volume` is a `FileStore` bound to one filesystem
//! handle; `MultiVolume` fans reads/exists/list out across all volumes so the
//! existing discovery/health code sees the whole machine unchanged.
//!
//! Chainloading uses a DEVICE PATH (volume device path + file path node), not a
//! memory buffer: firmware then loads the target from its own volume and sets its
//! `DeviceHandle`, so loaders like the Windows Boot Manager, GRUB, shim, and an
//! installer ISO's boot loader can find their own configuration and kernels
//! (UEFI spec §9; the classic `FileDevicePath(DeviceHandle, FilePath)` idiom).
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use uefi::{boot, Status, Handle, Identify};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::proto::media::file::Directory;
use uefi::proto::device_path::DevicePath;
use uefi::proto::device_path::build::{DevicePathBuilder, media::FilePath};
use uefi::proto::loaded_image::LoadedImage;
use ports::{DirEntryMeta, FileStore, IoError, IoResult};

use crate::fs::{read_in, read_head_in, write_in, exists_in, list_dir_in, list_dir_meta_in, to_cpath, map_err};

/// A `FileStore` bound to one filesystem handle. Stateless like `UefiFileStore`:
/// it reopens the volume root per operation. Its stable id is computed once, when
/// the volume is enumerated.
pub struct Volume {
    handle: Handle,
    id: String,
}

/// A stable, opaque id for a volume, derived from its device path (an FNV-1a
/// hash of the path nodes' data). Same hardware topology → same id across
/// reboots, so it disambiguates the SAME loader path on different disks and
/// lets the engine chainload from the right one. Falls back to a fixed string
/// if the device path can't be read (all such volumes then share one id).
/// The output must stay byte-identical: `EntryId`s persisted in NVRAM (e.g.
/// `LastGood`) end in `@<this id>`.
fn compute_id(handle: Handle) -> String {
    match boot::open_protocol_exclusive::<DevicePath>(handle) {
        Ok(dp) => {
            let mut h: u64 = 0xcbf29ce4_84222325;
            for node in dp.node_iter() {
                for &b in node.data() {
                    h ^= b as u64;
                    h = h.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
            alloc::format!("vol{h:016x}")
        }
        Err(_) => String::from("vol-unknown"),
    }
}

impl Volume {
    fn open_root(&self) -> IoResult<Directory> {
        let mut sfs = boot::open_protocol_exclusive::<SimpleFileSystem>(self.handle)
            .map_err(|e| map_err(e.status()))?;
        sfs.open_volume().map_err(|e| map_err(e.status()))
    }

    /// Chainload `file_path` from THIS volume via its device path, so the target
    /// can locate its own files. Passes `options` as UTF-16 load options. Returns
    /// only on failure (on success the machine leaves MyBoot). Falls back to a
    /// buffer load for self-contained images if the device-path load is refused.
    pub fn chainload(&self, file_path: &str, options: &str) -> Status {
        match self.load_via_device_path(file_path) {
            Ok(image) => { set_options_and_start(image, options); Status::LOAD_ERROR }
            Err(_) => self.chainload_from_buffer(file_path, options),
        }
    }

    /// This volume's stable id (see `compute_id`), computed once at enumeration.
    pub fn id(&self) -> &str { &self.id }

    /// Build `volume_device_path + FilePath(file_path)` and LoadImage from it.
    fn load_via_device_path(&self, file_path: &str) -> Result<Handle, Status> {
        let dp = boot::open_protocol_exclusive::<DevicePath>(self.handle)
            .map_err(|e| e.status())?;
        let cpath = to_cpath(file_path).map_err(|_| Status::INVALID_PARAMETER)?;

        let mut buf: Vec<u8> = Vec::new();
        let mut builder = DevicePathBuilder::with_vec(&mut buf);
        for node in dp.node_iter() {
            builder = builder.push(&node).map_err(|_| Status::OUT_OF_RESOURCES)?;
        }
        let full = builder
            .push(&FilePath { path_name: &cpath }).map_err(|_| Status::OUT_OF_RESOURCES)?
            .finalize().map_err(|_| Status::OUT_OF_RESOURCES)?;

        boot::load_image(
            boot::image_handle(),
            boot::LoadImageSource::FromDevicePath {
                device_path: full,
                boot_policy: uefi::proto::BootPolicy::ExactMatch,
            },
        ).map_err(|e| e.status())
    }

    /// Fallback: read the image bytes off this volume and load FROM A BUFFER.
    /// Works for self-contained EFI applications (UEFI shell, EFI-stub kernels).
    fn chainload_from_buffer(&self, file_path: &str, options: &str) -> Status {
        let bytes = match self.read(file_path) { Ok(b) => b, Err(_) => return Status::NOT_FOUND };
        match boot::load_image(
            boot::image_handle(),
            boot::LoadImageSource::FromBuffer { buffer: &bytes, file_path: None },
        ) {
            Ok(image) => { set_options_and_start(image, options); Status::LOAD_ERROR }
            Err(e) => e.status(),
        }
    }
}

impl FileStore for Volume {
    fn read(&self, path: &str) -> IoResult<Vec<u8>> { read_in(&mut self.open_root()?, path) }
    fn write(&mut self, path: &str, data: &[u8]) -> IoResult<()> { write_in(&mut self.open_root()?, path, data, false) }
    fn append(&mut self, path: &str, data: &[u8]) -> IoResult<()> { write_in(&mut self.open_root()?, path, data, true) }
    fn exists(&self, path: &str) -> bool {
        match self.open_root() { Ok(mut r) => exists_in(&mut r, path), Err(_) => false }
    }
    fn list_dir(&self, path: &str) -> IoResult<Vec<String>> { list_dir_in(&mut self.open_root()?, path) }
    fn read_head(&self, path: &str, max_len: usize) -> IoResult<Vec<u8>> {
        read_head_in(&mut self.open_root()?, path, max_len)
    }
    fn list_dir_meta(&self, path: &str) -> IoResult<Vec<DirEntryMeta>> {
        list_dir_meta_in(&mut self.open_root()?, path)
    }
    fn volume_label(&self) -> Option<String> {
        use uefi::proto::media::file::{FileSystemVolumeLabel, File};
        use alloc::string::ToString;
        let mut root = self.open_root().ok()?;
        let info = root.get_boxed_info::<FileSystemVolumeLabel>().ok()?;
        let s = info.volume_label().to_string();
        let s = s.trim();
        if s.is_empty() { None } else { Some(String::from(s)) }
    }
}

/// Set UTF-16 load options on a freshly loaded image, then start it. Returns only
/// on failure (a successful start never returns).
fn set_options_and_start(image: Handle, options: &str) {
    let opts16: Vec<u16> = if options.is_empty() {
        Vec::new()
    } else {
        options.encode_utf16().chain(core::iter::once(0)).collect()
    };
    if !opts16.is_empty() {
        if let Ok(mut li) = boot::open_protocol_exclusive::<LoadedImage>(image) {
            // SAFETY: opts16 outlives start_image below; size is in bytes.
            unsafe { li.set_load_options(opts16.as_ptr() as *const u8, (opts16.len() * 2) as u32); }
        }
    }
    let _ = boot::start_image(image);
    drop(opts16);
}

/// Force the firmware to bind filesystem drivers to EVERY partition, so all ESPs
/// become visible as `SimpleFileSystem`. Many firmwares only connect the volume the
/// running application was loaded from, so without this `volumes()` returns just one
/// ESP (and other OSes' loaders — e.g. Arch on a second ESP — are never found).
/// This mirrors what rEFInd and systemd-boot do. Best-effort and total: errors on
/// non-controller handles are ignored.
pub fn connect_all_controllers() {
    if let Ok(handles) = boot::locate_handle_buffer(boot::SearchType::AllHandles) {
        for &handle in handles.iter() {
            // Recursive so child controllers (partitions under a disk) are bound too.
            let _ = boot::connect_controller(handle, None, None, true);
        }
    }
}

/// All filesystems the firmware knows about, each as a `Volume`.
pub fn volumes() -> Vec<Volume> {
    match boot::locate_handle_buffer(boot::SearchType::ByProtocol(&SimpleFileSystem::GUID)) {
        Ok(handles) => handles.iter().map(|&handle| Volume { handle, id: compute_id(handle) }).collect(),
        Err(_) => Vec::new(),
    }
}

/// The stable id of the volume MyBoot itself was loaded from, if it can be
/// determined. Used for self-exclusion: MyBoot must not offer to boot its OWN
/// loader. Best-effort — returns `None` if the loaded-image device is unknown, in
/// which case the caller simply skips exclusion (shows an extra entry rather than
/// risk hiding a real OS).
pub fn own_volume_id() -> Option<String> {
    use uefi::proto::loaded_image::LoadedImage;
    let li = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle()).ok()?;
    let device = li.device()?;
    Some(compute_id(device))
}

/// A `FileStore` that fans out across EVERY volume, so the existing single-store
/// discovery and health code sees the whole machine. Reads resolve on the first
/// volume that has the path; `exists` is true if any volume has it; `list_dir`
/// unions all volumes (deduped). Writes go to the boot volume (MyBoot's own ESP).
pub struct MultiVolume {
    vols: Vec<Volume>,
    boot: crate::fs::UefiFileStore,
}

impl MultiVolume {
    /// Enumerate all volumes. First connects every controller so ALL ESPs are
    /// exposed (not just the pre-connected boot volume), then always includes the
    /// boot volume for writes.
    pub fn discover() -> Self {
        connect_all_controllers();
        Self::enumerate()
    }

    /// Enumerate filesystems WITHOUT connecting controllers first. The engine
    /// calls `connect_all_controllers()` itself before this so the two steps can
    /// be timed separately; `discover()` is the combined form.
    pub fn enumerate() -> Self {
        MultiVolume { vols: volumes(), boot: crate::fs::UefiFileStore::new() }
    }

    /// Number of volumes discovered (for diagnostics).
    pub fn len(&self) -> usize { self.vols.len() }
    pub fn is_empty(&self) -> bool { self.vols.is_empty() }

    /// Per-volume scan targets for volume-scoped discovery: each volume paired
    /// with its stable id, as a `FileStore`. `discovery::discover_multi` tags every
    /// entry it finds on a volume with that id, so two disks that share a loader
    /// path stay distinct.
    pub fn scan_targets(&self) -> Vec<(String, &dyn FileStore)> {
        self.vols.iter().map(|v| (String::from(v.id()), v as &dyn FileStore)).collect()
    }

    /// Chainload `file_path` from the SPECIFIC volume identified by `volume_id`
    /// (the id carried by the chosen entry), so the right disk's copy is booted —
    /// e.g. a Ventoy USB's `\EFI\BOOT\BOOTX64.EFI` rather than MyBoot's own. When
    /// no volume is known (a firmware-variable entry), falls back to scanning all
    /// volumes for the path. Returns only on failure.
    pub fn chainload_on(&self, volume_id: Option<&str>, file_path: &str, options: &str) -> Status {
        if let Some(vid) = volume_id {
            for v in &self.vols {
                if v.id() == vid && v.exists(file_path) {
                    return v.chainload(file_path, options); // never returns on success
                }
            }
        }
        self.chainload(file_path, options)
    }

    /// Chainload `file_path` from whichever volume holds it, via device path.
    /// Returns only on failure.
    pub fn chainload(&self, file_path: &str, options: &str) -> Status {
        for v in &self.vols {
            if v.exists(file_path) {
                return v.chainload(file_path, options); // never returns on success
            }
        }
        Status::NOT_FOUND
    }
}

impl FileStore for MultiVolume {
    fn read(&self, path: &str) -> IoResult<Vec<u8>> {
        for v in &self.vols {
            if let Ok(bytes) = v.read(path) { return Ok(bytes); }
        }
        Err(IoError::NotFound)
    }
    fn write(&mut self, path: &str, data: &[u8]) -> IoResult<()> { self.boot.write(path, data) }
    fn append(&mut self, path: &str, data: &[u8]) -> IoResult<()> { self.boot.append(path, data) }
    fn exists(&self, path: &str) -> bool { self.vols.iter().any(|v| v.exists(path)) }
    fn list_dir(&self, path: &str) -> IoResult<Vec<String>> {
        let mut names: Vec<String> = Vec::new();
        let mut found = false;
        for v in &self.vols {
            if let Ok(list) = v.list_dir(path) {
                found = true;
                for n in list { if !names.iter().any(|x| x == &n) { names.push(n); } }
            }
        }
        if found { Ok(names) } else { Err(IoError::NotFound) }
    }
    fn read_head(&self, path: &str, max_len: usize) -> IoResult<Vec<u8>> {
        for v in &self.vols {
            if let Ok(bytes) = v.read_head(path, max_len) { return Ok(bytes); }
        }
        Err(IoError::NotFound)
    }
    /// Union by name across volumes; the first volume listing a name wins
    /// (mirrors `list_dir`).
    fn list_dir_meta(&self, path: &str) -> IoResult<Vec<DirEntryMeta>> {
        let mut items: Vec<DirEntryMeta> = Vec::new();
        let mut found = false;
        for v in &self.vols {
            if let Ok(list) = v.list_dir_meta(path) {
                found = true;
                for m in list { if !items.iter().any(|x| x.name == m.name) { items.push(m); } }
            }
        }
        if found { Ok(items) } else { Err(IoError::NotFound) }
    }
}
