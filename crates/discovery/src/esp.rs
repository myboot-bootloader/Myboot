//! Probe the ESP for well-known loaders, the way rEFInd scans for boot targets
//! (report §3.3). We look for the Windows boot manager and the removable-media
//! fallback loader; richer per-distro scanning is layered on by BLS/bootspec.
extern crate alloc;
use crate::RawEntry;
use alloc::vec::Vec;
use graph::{BootMethod, EntryRole, OsKind};
use ports::FileStore;

/// Canonical Windows loader path (verified fact: bootmgfw.efi, NOT bootmgr.efi).
pub const WINDOWS_LOADER: &str = "\\EFI\\Microsoft\\Boot\\bootmgfw.efi";
/// Removable-media fallback loader path.
pub const FALLBACK_LOADER: &str = "\\EFI\\BOOT\\BOOTX64.EFI";

pub fn discover<F: FileStore + ?Sized>(fs: &F) -> Vec<RawEntry> {
    let mut out = Vec::new();

    if fs.exists(WINDOWS_LOADER) {
        let mut e = RawEntry::new(
            OsKind::Windows,
            "Windows",
            "Windows Boot Manager",
            EntryRole::Default,
            BootMethod::WindowsBootManager,
        );
        e.loader = Some(WINDOWS_LOADER.into());
        out.push(e);
    }

    if fs.exists(FALLBACK_LOADER) {
        // Try to give it the REAL distro name (so the menu reads "Ubuntu 26.04.1"
        // not "EFI Fallback Loader"). Debian-family live/installer ISOs — Ubuntu,
        // Parrot, Mint, Kali, Zorin, Tails — carry their product string in
        // `.disk/info`. If we can't identify it, the title stays generic and the
        // volume tag added later keeps multiple entries distinct.
        let (os_name, title) = match media_name(fs) {
            Some(name) => (name.clone(), name),
            None => (
                alloc::string::String::from("EFI"),
                alloc::string::String::from("EFI Fallback Loader"),
            ),
        };
        let mut e = RawEntry::new(
            OsKind::GenericEfi,
            os_name,
            title,
            EntryRole::Fallback,
            BootMethod::EfiChainload,
        );
        e.loader = Some(FALLBACK_LOADER.into());
        out.push(e);
    }

    out
}

/// Read a human-friendly product name off THIS volume from sources that live on
/// the FAT EFI partition MyBoot can actually read (an installer ISO's `.disk/info`
/// is on the ISO9660 side and usually NOT here). Priority: an explicit product
/// file, then the distro named in the EFI `grub.cfg`, then the FAT volume label.
/// Pure and total — any read/parse failure just falls through.
fn media_name<F: FileStore + ?Sized>(fs: &F) -> Option<alloc::string::String> {
    // 1. A `BOOT.CSV` next to the loader carries a clean label (shim writes e.g.
    //    `shimx64.efi,ubuntu,,This is the boot entry for ubuntu`). This is what
    //    rEFInd reads for installed systems.
    for path in ["\\EFI\\BOOT\\BOOT.CSV", "\\EFI\\BOOT\\BOOTX64.CSV"] {
        if let Ok(bytes) = fs.read(path) {
            if let Some(name) = label_from_boot_csv(&bytes) {
                return Some(name);
            }
        }
    }
    // 2. Debian-family product file, if the whole ISO happens to be exposed.
    for path in ["\\.disk\\info", "\\.disk\\info.txt"] {
        if let Ok(bytes) = fs.read(path) {
            if let Some(name) = clean_product_name(&bytes) {
                return Some(name);
            }
        }
    }
    // 3. The grub config, if it's on this volume, names the distro.
    for path in [
        "\\EFI\\BOOT\\grub.cfg",
        "\\boot\\grub\\grub.cfg",
        "\\EFI\\BOOT\\grub\\grub.cfg",
    ] {
        if let Ok(bytes) = fs.read(path) {
            if let Some(name) = distro_from_grub_cfg(&bytes) {
                return Some(name);
            }
        }
    }
    // 4. Live ISOs have NO config on their EFI partition (only the loaders), so
    //    identify the distro from the loader BINARIES. GRUB is checked before
    //    shim: GRUB is the distro's own build, while shim is often reused from
    //    the distro it is based on (Parrot ships Debian's signed shim, so a
    //    shim-first scan named it "Debian"). First the SBAT metadata (exact vendor
    //    ids, see `sbat`); only if no SBAT names a known distro, fall back — like
    //    rEFInd's icon detection — to a marker substring scan, in the same order.
    let loaders: alloc::vec::Vec<alloc::vec::Vec<u8>> = LOADER_SCAN_ORDER
        .iter()
        .filter_map(|path| fs.read(path).ok())
        .collect();
    for bytes in &loaders {
        if let Ok(section) = storage::pe::section::find(bytes, b".sbat") {
            if let Some(name) = crate::sbat::distro_from_sbat(section) {
                return Some(name);
            }
        }
    }
    for bytes in &loaders {
        if let Some(name) = distro_from_bytes(bytes) {
            return Some(name);
        }
    }
    // 5. The FAT volume label (the "ISO volume label"), if it's a real name.
    if let Some(label) = fs.volume_label() {
        let l = label.trim();
        let generic = [
            "",
            "ESP",
            "EFI",
            "BOOT",
            "CDROM",
            "ISOIMAGE",
            "SYSTEM",
            "QEMU VVFAT",
        ];
        if !generic.iter().any(|g| l.eq_ignore_ascii_case(g)) && l.len() >= 2 {
            return Some(l.chars().take(48).collect());
        }
    }
    None
}

/// Decode text that may be UTF-8 or UTF-16LE (shim writes BOOT.CSV as UTF-16 with
/// a BOM).
fn decode_text(bytes: &[u8]) -> Option<alloc::string::String> {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        // `as_chunks` yields only whole 2-byte units; an odd tail is dropped.
        let u16s: alloc::vec::Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        return Some(alloc::string::String::from_utf16_lossy(&u16s));
    }
    core::str::from_utf8(bytes)
        .ok()
        .map(alloc::string::String::from)
}

/// A `BOOT.CSV` line is `loader,label,options,description`; the label is the name.
fn label_from_boot_csv(bytes: &[u8]) -> Option<alloc::string::String> {
    let text = decode_text(bytes)?;
    let line = text.lines().next()?.trim().trim_start_matches('\u{feff}');
    let label = line.split(',').nth(1)?.trim();
    if label.is_empty() {
        return None;
    }
    // If it's a bare distro id ("ubuntu"), prettify via the marker table.
    let lower = label.to_ascii_lowercase();
    for (marker, name) in DISTRO_MARKERS {
        if lower == *marker {
            return Some(alloc::string::String::from(*name));
        }
    }
    Some(label.chars().take(48).collect())
}

/// Scan a loader binary's bytes for the first known distro marker (ASCII,
/// case-insensitive). This is how rEFInd guesses an OS from a bare loader.
fn distro_from_bytes(bytes: &[u8]) -> Option<alloc::string::String> {
    for (marker, name) in DISTRO_MARKERS {
        if contains_ascii_ci(bytes, marker.as_bytes()) {
            return Some(alloc::string::String::from(*name));
        }
    }
    None
}

/// Case-insensitive ASCII substring search over raw bytes.
fn contains_ascii_ci(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    hay.windows(needle.len())
        .any(|w| w.iter().zip(needle).all(|(a, b)| a.eq_ignore_ascii_case(b)))
}

/// Loader binaries `media_name` inspects, most distro-specific first: the
/// distro's own GRUB, then the fallback loader (often shim), then shim.
const LOADER_SCAN_ORDER: [&str; 3] = [
    "\\EFI\\BOOT\\grubx64.efi",
    "\\EFI\\BOOT\\BOOTX64.EFI",
    "\\EFI\\BOOT\\shimx64.efi",
];

/// Known distro markers to look for in a grub.cfg. Ordered so specific spins are
/// matched before their base (Parrot/Kali before Debian; archiso before arch).
pub(crate) const DISTRO_MARKERS: &[(&str, &str)] = &[
    ("parrot", "Parrot OS"),
    ("kali", "Kali Linux"),
    ("linuxmint", "Linux Mint"),
    ("zorin", "Zorin OS"),
    ("tails", "Tails"),
    ("pop!_os", "Pop!_OS"),
    ("pop_os", "Pop!_OS"),
    ("elementary", "elementary OS"),
    ("manjaro", "Manjaro"),
    ("endeavour", "EndeavourOS"),
    ("nixos", "NixOS"),
    ("fedora", "Fedora"),
    ("casper", "Ubuntu"), // Ubuntu's live boot dir
    ("ubuntu", "Ubuntu"),
    ("archiso", "Arch Linux"),
    ("archlinux", "Arch Linux"),
    ("debian", "Debian"),
];

/// Identify the distro from the text of a grub.cfg by the first known marker.
fn distro_from_grub_cfg(bytes: &[u8]) -> Option<alloc::string::String> {
    let text = core::str::from_utf8(bytes).ok()?;
    let lower = text.to_ascii_lowercase();
    for (marker, name) in DISTRO_MARKERS {
        if lower.contains(marker) {
            return Some(alloc::string::String::from(*name));
        }
    }
    None
}

/// Turn the first line of a product-info file into a clean, short menu title:
/// take the text before the first `"` codename or ` - ` release suffix, trim, and
/// cap the length. Total on any input.
fn clean_product_name(bytes: &[u8]) -> Option<alloc::string::String> {
    let text = core::str::from_utf8(bytes).ok()?;
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let name = line.split('"').next().unwrap_or(line);
    let name = name.split(" - ").next().unwrap_or(name).trim();
    if name.is_empty() {
        return None;
    }
    Some(name.chars().take(48).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ports::mock::MockFileStore;

    #[test]
    fn finds_windows_when_present() {
        let fs = MockFileStore::new().with_file(WINDOWS_LOADER, b"MZ");
        let e = discover(&fs);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].kind, OsKind::Windows);
        assert_eq!(e[0].loader.as_deref(), Some(WINDOWS_LOADER));
    }

    #[test]
    fn empty_esp_yields_nothing() {
        assert!(discover(&MockFileStore::new()).is_empty());
    }

    #[test]
    fn names_the_fallback_from_disk_info() {
        let info = b"Ubuntu 26.04.1 LTS \"Plucky Puffin\" - Release amd64 (20260421)\n";
        let fs = MockFileStore::new()
            .with_file(FALLBACK_LOADER, b"MZ")
            .with_file("\\.disk\\info", info);
        let e = discover(&fs);
        assert_eq!(e.len(), 1);
        assert_eq!(
            e[0].title, "Ubuntu 26.04.1 LTS",
            "clean product name, no codename/release suffix"
        );
        assert_eq!(e[0].loader.as_deref(), Some(FALLBACK_LOADER));
    }

    #[test]
    fn falls_back_to_generic_title_without_disk_info() {
        let fs = MockFileStore::new().with_file(FALLBACK_LOADER, b"MZ");
        let e = discover(&fs);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].title, "EFI Fallback Loader");
    }

    #[test]
    fn names_ubuntu_from_grub_cfg() {
        // The EFI-partition grub.cfg names the distro via a menuentry / casper dir.
        let cfg = b"menuentry \"Try or Install Ubuntu\" {\n  linux /casper/vmlinuz ---\n}\n";
        let fs = MockFileStore::new()
            .with_file(FALLBACK_LOADER, b"MZ")
            .with_file("\\EFI\\BOOT\\grub.cfg", cfg);
        let e = discover(&fs);
        assert_eq!(e[0].title, "Ubuntu");
    }

    #[test]
    fn names_parrot_from_grub_cfg_over_debian_base() {
        // Parrot is Debian-based; the specific marker must win over "debian".
        let cfg =
            b"# Parrot home Live Boot Menu\nmenuentry 'Parrot' { linux /live/vmlinuz boot=live }\n";
        let fs = MockFileStore::new()
            .with_file(FALLBACK_LOADER, b"MZ")
            .with_file("\\boot\\grub\\grub.cfg", cfg);
        let e = discover(&fs);
        assert_eq!(e[0].title, "Parrot OS");
    }

    #[test]
    fn names_from_boot_csv_label() {
        // shim's BOOT.CSV: loader,label,options,description
        let csv = b"shimx64.efi,ubuntu,,This is the boot entry for ubuntu\n";
        let fs = MockFileStore::new()
            .with_file(FALLBACK_LOADER, b"MZ")
            .with_file("\\EFI\\BOOT\\BOOT.CSV", csv);
        let e = discover(&fs);
        assert_eq!(
            e[0].title, "Ubuntu",
            "bare 'ubuntu' label prettified via marker table"
        );
    }

    #[test]
    fn names_by_scanning_the_loader_binary() {
        // No config, no CSV — only the loader, which embeds a distro string.
        let mut loader = alloc::vec::Vec::from(*b"MZ\x90\x00 GRUB image ");
        loader.extend_from_slice(b"prefix=(cd0)/casper stuff \x00\x01");
        let fs = MockFileStore::new().with_file(FALLBACK_LOADER, &loader);
        let e = discover(&fs);
        assert_eq!(
            e[0].title, "Ubuntu",
            "casper marker in the binary maps to Ubuntu"
        );
    }

    /// A minimal PE image whose only section is `.sbat` holding `sbat`, followed
    /// by `tail` (stand-in for the rest of the binary, e.g. embedded strings).
    fn pe_with_sbat(sbat: &[u8], tail: &[u8]) -> alloc::vec::Vec<u8> {
        let (pe_off, opt_size) = (0x80usize, 0xF0usize);
        let coff = pe_off + 4;
        let table = coff + 20 + opt_size;
        let data_at = table + 40;
        let mut b = alloc::vec![0u8; data_at];
        b[0] = b'M';
        b[1] = b'Z';
        b[0x3C..0x40].copy_from_slice(&(pe_off as u32).to_le_bytes());
        b[pe_off..pe_off + 4].copy_from_slice(b"PE\0\0");
        b[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
        b[coff + 16..coff + 18].copy_from_slice(&(opt_size as u16).to_le_bytes());
        b[table..table + 5].copy_from_slice(b".sbat");
        b[table + 8..table + 12].copy_from_slice(&(sbat.len() as u32).to_le_bytes());
        b[table + 16..table + 20].copy_from_slice(&(sbat.len() as u32).to_le_bytes());
        b[table + 20..table + 24].copy_from_slice(&(data_at as u32).to_le_bytes());
        b.extend_from_slice(sbat);
        b.extend_from_slice(tail);
        b
    }

    #[test]
    fn parrot_live_iso_is_named_from_grub_sbat_not_debians_shim() {
        // The real Parrot 7.3 layout: BOOTX64.EFI is Debian's signed shim, the
        // Parrot identity is only in grubx64.efi. A shim-first substring scan
        // named this "Debian".
        let shim = pe_with_sbat(
            b"sbat,1,SBAT Version,sbat,1,x\nshim.debian,1,Debian,shim,15.8,x\n",
            b" Debian Secure Boot CA ",
        );
        let grub = pe_with_sbat(b"sbat,1,x\ngrub.debian,5,Debian,grub2,2.14-2parrot3,x\n\
grub.debian14,1,Debian,grub2,2.14-2parrot3,x\ngrub.parrot,1,Parrot Security,grub2,2.14-2parrot3,x\n", b"");
        let fs = MockFileStore::new()
            .with_file(FALLBACK_LOADER, &shim)
            .with_file("\\EFI\\BOOT\\grubx64.efi", &grub)
            .with_file(
                "\\boot\\grub\\grub.cfg",
                b"search --set=root --file /.disk/info\nconfigfile ($root)/boot/grub/grub.cfg\n",
            );
        assert_eq!(discover(&fs)[0].title, "Parrot OS");
    }

    #[test]
    fn ubuntu_live_iso_is_named_from_grub_sbat() {
        let shim = pe_with_sbat(
            b"sbat,1,x\nshim.ubuntu,1,Ubuntu,shim,15.8-0ubuntu2,x\n",
            b"",
        );
        let grub = pe_with_sbat(
            b"sbat,1,x\ngrub.ubuntu,2,Ubuntu,grub2,2.14-2ubuntu1,x\n\
grub.peimage,2,Canonical,grub2,2.14-2ubuntu1,x\n",
            b"",
        );
        let fs = MockFileStore::new()
            .with_file(FALLBACK_LOADER, &shim)
            .with_file("\\EFI\\BOOT\\grubx64.efi", &grub);
        assert_eq!(discover(&fs)[0].title, "Ubuntu");
    }

    #[test]
    fn sbat_beats_substring_false_positives() {
        // "details" contains the marker "tails", which outranks "debian" in the
        // substring table. SBAT names the vendor exactly, so it wins.
        let loader = pe_with_sbat(
            b"sbat,1,x\nshim.debian,1,Debian,shim,15.8,x\n",
            b" see details at https://example.org ",
        );
        let fs = MockFileStore::new().with_file(FALLBACK_LOADER, &loader);
        assert_eq!(discover(&fs)[0].title, "Debian");
    }
}
