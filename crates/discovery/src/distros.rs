//! Linux distribution discovery by ESP vendor directory (report §3.3). The UEFI
//! ESP has a standard layout: each OS installs its loader under `\EFI\<vendor>\`
//! (Gentoo wiki "EFI System Partition"; efibootmgr docs). We scan `\EFI\` for
//! known distro directories and, for each, chainload its signed loader —
//! `shimx64.efi` (preferred under Secure Boot) or `grubx64.efi`. This is exactly
//! how firmware boot menus and rEFInd enumerate installed Linux systems.
//!
//! Where a distro ships an `os-release` file on the ESP we read `PRETTY_NAME`
//! for the exact edition; otherwise the vendor directory gives the family name.
extern crate alloc;
use crate::RawEntry;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use graph::{BootMethod, EntryRole, OsKind};
use ports::FileStore;

pub const EFI_DIR: &str = "\\EFI";

/// Loader file names to try, in order of preference: shim first (it is what a
/// Secure Boot chain launches), then GRUB, then systemd-boot.
const LOADERS: &[&str] = &[
    "shimx64.efi",
    "grubx64.efi",
    "systemd-bootx64.efi",
    "bootx64.efi",
];

/// Known distro vendor-directory names → their display names. Directory matching
/// is case-insensitive (the ESP is FAT: case-preserving, case-insensitive).
/// Covers the common families and the ones commonly multi-booted.
const KNOWN: &[(&str, &str)] = &[
    ("ubuntu", "Ubuntu"),
    ("kubuntu", "Kubuntu"),
    ("xubuntu", "Xubuntu"),
    ("lubuntu", "Lubuntu"),
    ("debian", "Debian"),
    ("kali", "Kali Linux"),
    ("parrot", "Parrot OS"),
    ("zorin", "Zorin OS"),
    ("linuxmint", "Linux Mint"),
    ("mint", "Linux Mint"),
    ("pop", "Pop!_OS"),
    ("elementary", "elementary OS"),
    ("fedora", "Fedora"),
    ("redhat", "Red Hat"),
    ("rhel", "Red Hat"),
    ("centos", "CentOS"),
    ("rocky", "Rocky Linux"),
    ("almalinux", "AlmaLinux"),
    ("opensuse", "openSUSE"),
    ("suse", "SUSE"),
    ("sles", "SUSE"),
    ("arch", "Arch Linux"),
    ("archlinux", "Arch Linux"),
    ("endeavouros", "EndeavourOS"),
    ("manjaro", "Manjaro"),
    ("garuda", "Garuda Linux"),
    ("gentoo", "Gentoo"),
    ("nixos", "NixOS"),
    ("void", "Void Linux"),
    ("mx", "MX Linux"),
    ("mageia", "Mageia"),
    ("solus", "Solus"),
    ("slackware", "Slackware"),
    ("alpine", "Alpine Linux"),
    ("clearlinux", "Clear Linux"),
];

/// Scan the ESP for installed distributions.
pub fn discover<F: FileStore + ?Sized>(fs: &F) -> Vec<RawEntry> {
    let mut out = Vec::new();
    let dirs = match fs.list_dir(EFI_DIR) {
        Ok(d) => d,
        Err(_) => return out,
    };

    for dir in dirs {
        // Skip the vendor dirs handled by other sources / not Linux distros.
        let low = dir.to_ascii_lowercase();
        if matches!(
            low.as_str(),
            "boot" | "microsoft" | "systemd" | "refind" | "nixos" | "myboot"
        ) {
            continue; // BOOT=fallback, Microsoft=Windows, nixos handled by bootspec
        }

        // Find a loader inside this vendor directory.
        let loader = LOADERS.iter().find_map(|name| {
            let path = format!("{EFI_DIR}\\{dir}\\{name}");
            if fs.exists(&path) {
                Some(path)
            } else {
                None
            }
        });
        let loader = match loader {
            Some(l) => l,
            None => continue,
        };

        // Resolve the display name: known table → PRETTY_NAME on ESP → dir name.
        let (kind, name) = classify(&low, &dir);
        let title = pretty_name_on_esp(fs, &dir).unwrap_or_else(|| name.clone());

        let mut e = RawEntry::new(
            kind,
            name,
            title,
            EntryRole::Default,
            BootMethod::EfiChainload,
        );
        e.loader = Some(loader);
        out.push(e);
    }
    out
}

/// Classify a vendor directory into (OsKind, display name).
fn classify(low: &str, original: &str) -> (OsKind, String) {
    for (slug, name) in KNOWN {
        if low == *slug {
            return (OsKind::Linux, (*name).to_string());
        }
    }
    // Unknown vendor dir that nonetheless holds a loader: still a Linux/EFI OS.
    (OsKind::Linux, capitalise(original))
}

/// Read `PRETTY_NAME` from an `os-release` file in the vendor dir, if present.
/// Some installs place a copy on the ESP; when absent we fall back to the family.
fn pretty_name_on_esp<F: FileStore + ?Sized>(fs: &F, dir: &str) -> Option<String> {
    for candidate in ["os-release", "etc/os-release"] {
        let path = format!("{EFI_DIR}\\{dir}\\{candidate}");
        if let Ok(bytes) = fs.read(&path) {
            if let Ok(text) = core::str::from_utf8(&bytes) {
                if let Some(name) = parse_pretty_name(text) {
                    return Some(name);
                }
            }
        }
    }
    None
}

/// Extract `PRETTY_NAME="..."` from os-release content. Total.
pub fn parse_pretty_name(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
            let v = rest.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ports::mock::MockFileStore;

    #[test]
    fn detects_known_distros_and_prefers_shim() {
        let fs = MockFileStore::new()
            .with_file("\\EFI\\ubuntu\\shimx64.efi", b"MZ")
            .with_file("\\EFI\\ubuntu\\grubx64.efi", b"MZ")
            .with_file("\\EFI\\kali\\grubx64.efi", b"MZ")
            .with_file("\\EFI\\Microsoft\\Boot\\bootmgfw.efi", b"MZ"); // ignored here
        let entries = discover(&fs);
        let ubuntu = entries.iter().find(|e| e.title == "Ubuntu").unwrap();
        assert_eq!(
            ubuntu.loader.as_deref(),
            Some("\\EFI\\ubuntu\\shimx64.efi"),
            "shim preferred"
        );
        assert!(entries.iter().any(|e| e.title == "Kali Linux"));
        assert!(!entries.iter().any(|e| e.title.contains("Microsoft")));
        assert!(entries
            .iter()
            .all(|e| e.kind == OsKind::Linux && e.method == BootMethod::EfiChainload));
    }

    #[test]
    fn reads_pretty_name_from_os_release_when_present() {
        let fs = MockFileStore::new()
            .with_file("\\EFI\\zorin\\grubx64.efi", b"MZ")
            .with_file(
                "\\EFI\\zorin\\os-release",
                b"NAME=Zorin\nPRETTY_NAME=\"Zorin OS 17.1\"\n",
            );
        let entries = discover(&fs);
        let z = entries
            .iter()
            .find(|e| e.loader.as_deref() == Some("\\EFI\\zorin\\grubx64.efi"))
            .unwrap();
        assert_eq!(z.title, "Zorin OS 17.1", "exact edition from PRETTY_NAME");
    }

    #[test]
    fn unknown_vendor_dir_with_loader_is_still_linux() {
        let fs = MockFileStore::new().with_file("\\EFI\\frobos\\grubx64.efi", b"MZ");
        let entries = discover(&fs);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Frobos");
        assert_eq!(entries[0].kind, OsKind::Linux);
    }

    #[test]
    fn vendor_dir_without_a_loader_is_skipped() {
        let fs = MockFileStore::new().with_file("\\EFI\\ubuntu\\README", b"x");
        assert!(discover(&fs).is_empty());
    }

    #[test]
    fn parse_pretty_name_is_total() {
        assert_eq!(
            parse_pretty_name("PRETTY_NAME=\"Kali GNU/Linux Rolling\"").as_deref(),
            Some("Kali GNU/Linux Rolling")
        );
        assert_eq!(parse_pretty_name("NAME=Debian\n"), None);
        assert_eq!(parse_pretty_name(""), None);
    }
}
