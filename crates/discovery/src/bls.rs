//! Boot Loader Specification Type #1 entries — the `\loader\entries\*.conf`
//! format used by systemd-boot and others (report §3.3). Each `.conf` is a set
//! of `key value` lines; we read `title`, `version`, `linux`, `initrd`,
//! `options`, and `sort-key`. Parsing is total: a malformed file is skipped.
extern crate alloc;
use crate::RawEntry;
use alloc::string::String;
use alloc::vec::Vec;
use graph::{BootMethod, EntryRole, OsKind};
use ports::FileStore;

pub const ENTRIES_DIR: &str = "\\loader\\entries";

pub fn discover<F: FileStore + ?Sized>(fs: &F) -> Vec<RawEntry> {
    let mut out = Vec::new();
    let names = match fs.list_dir(ENTRIES_DIR) {
        Ok(n) => n,
        Err(_) => return out,
    };
    for name in names {
        if !name.ends_with(".conf") {
            continue;
        }
        let mut path = String::from(ENTRIES_DIR);
        path.push('\\');
        path.push_str(&name);
        let bytes = match fs.read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if let Some(e) = parse_conf(&bytes) {
            out.push(e);
        }
    }
    out
}

/// Parse one BLS conf. Returns None if it lacks a usable kernel (`linux`).
pub fn parse_conf(bytes: &[u8]) -> Option<RawEntry> {
    let text = core::str::from_utf8(bytes).ok()?;
    let mut title: Option<String> = None;
    let mut version: Option<String> = None;
    let mut linux: Option<String> = None;
    let mut initrd: Option<String> = None;
    let mut options: Option<String> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // split on the first run of whitespace: `key value...`
        let (key, val) = match line.split_once(char::is_whitespace) {
            Some((k, v)) => (k, v.trim()),
            None => continue,
        };
        match key {
            "title" => title = Some(val.into()),
            "version" => version = Some(val.into()),
            "linux" => linux = Some(to_efi_path(val)),
            "initrd" => initrd = Some(to_efi_path(val)),
            "options" => options = Some(val.into()),
            _ => {} // ignore machine-id, sort-key, architecture, devicetree, ...
        }
    }

    let linux = linux?; // a Type #1 entry without a kernel is not bootable

    // NixOS + systemd-boot encodes the generation and build date in the `version`
    // line (e.g. "Generation 23 NixOS ... (Linux 7.1.8), built on 2026-09-01"),
    // while `title` is just "NixOS" for every generation. Recognise that so each
    // generation gets a DISTINCT, informative title and they group under NixOS —
    // instead of a wall of identical "NixOS" rows.
    let base = title.clone().unwrap_or_else(|| String::from("Linux"));
    let (kind, os_name, role, display) = match version.as_deref().and_then(nixos_generation) {
        Some(g) => (
            OsKind::NixOs,
            "NixOS",
            EntryRole::Generation(g),
            version.clone().unwrap_or(base), // the version line is self-describing
        ),
        None => {
            // Generic Linux/BLS: fold a distinguishing `version` into the title so
            // multiple kernels of the same OS don't all read the same.
            let display = match &version {
                Some(v) if base != *v && !base.contains(v.as_str()) => {
                    alloc::format!("{base} \u{2014} {v}")
                }
                _ => base,
            };
            let role = match &version {
                Some(v) => EntryRole::Kernel(v.clone()),
                None => EntryRole::Default,
            };
            (OsKind::Linux, "Linux", role, display)
        }
    };

    let mut e = RawEntry::new(kind, os_name, display, role, BootMethod::LinuxEfiStub);
    e.kernel = Some(linux);
    e.initrd = initrd;
    e.cmdline = options;
    Some(e)
}

/// Parse the generation number out of a NixOS `version` line, e.g.
/// "Generation 23 NixOS Zokor 26.11 ... built on 2026-09-01" -> 23.
fn nixos_generation(version: &str) -> Option<u32> {
    let rest = version.strip_prefix("Generation ")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok()
}

/// BLS paths are ESP-relative with `/`; normalise to EFI `\` form.
fn to_efi_path(p: &str) -> String {
    let mut s = String::with_capacity(p.len());
    for c in p.chars() {
        s.push(if c == '/' { '\\' } else { c });
    }
    if !s.starts_with('\\') {
        s.insert(0, '\\');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use ports::mock::MockFileStore;

    const CONF: &str = "title Fedora 39\nversion 6.6.0\nlinux /6a/linux\ninitrd /6a/initrd\noptions root=UUID=abc ro\n";

    #[test]
    fn parses_a_type1_entry() {
        let e = parse_conf(CONF.as_bytes()).unwrap();
        assert_eq!(e.kind, OsKind::Linux);
        assert_eq!(e.title, "Fedora 39 \u{2014} 6.6.0"); // version folded in for distinctness
        assert_eq!(e.kernel.as_deref(), Some("\\6a\\linux"));
        assert_eq!(e.initrd.as_deref(), Some("\\6a\\initrd"));
        assert_eq!(e.cmdline.as_deref(), Some("root=UUID=abc ro"));
        assert_eq!(e.role, EntryRole::Kernel("6.6.0".into()));
    }

    #[test]
    fn entry_without_kernel_is_rejected() {
        assert!(parse_conf(b"title Broken\noptions ro\n").is_none());
    }

    #[test]
    fn nixos_generation_gets_a_distinct_title_and_groups_under_nixos() {
        // What NixOS + systemd-boot actually writes: title is "NixOS" for every
        // generation, the distinguishing info is in `version`.
        let conf = "title NixOS\nversion Generation 23 NixOS Zokor 26.11.20260810.2fcb964 (Linux 7.1.8), built on 2026-09-01\nlinux /efi/nixos/abc-linux.efi\ninitrd /efi/nixos/abc-initrd.efi\noptions init=/nix/store/xxx/init\nsort-key nixos\n";
        let e = parse_conf(conf.as_bytes()).unwrap();
        assert_eq!(
            e.kind,
            OsKind::NixOs,
            "recognised as NixOS, not generic Linux"
        );
        assert_eq!(e.role, EntryRole::Generation(23));
        assert!(
            e.title.contains("Generation 23"),
            "title carries the generation"
        );
        assert!(
            e.title.contains("2026-09-01"),
            "title carries the build date: {}",
            e.title
        );
    }

    #[test]
    fn two_nixos_generations_are_distinct() {
        let g23 = parse_conf(b"title NixOS\nversion Generation 23 NixOS (Linux 7.1.8), built on 2026-09-01\nlinux /a/l\n").unwrap();
        let g22 = parse_conf(b"title NixOS\nversion Generation 22 NixOS (Linux 7.1.8), built on 2026-08-22\nlinux /b/l\n").unwrap();
        assert_ne!(g23.title, g22.title);
        assert_ne!(g23.role, g22.role);
    }

    #[test]
    fn generic_version_is_folded_into_the_title() {
        let e = parse_conf(CONF.as_bytes()).unwrap(); // Fedora 39 / version 6.6.0
        assert_eq!(e.kind, OsKind::Linux);
        assert!(
            e.title.contains("Fedora 39") && e.title.contains("6.6.0"),
            "title: {}",
            e.title
        );
    }
}
