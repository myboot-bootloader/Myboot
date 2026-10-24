//! NixOS bootspec discovery (RFC 0125), the mechanism Lanzaboote and modern
//! NixOS use to describe each generation (report §3.3: NixOS generations). Each
//! generation directory holds a `bootspec.json` (or `boot.json`) with the
//! `org.nixos.bootspec.v1` object and optional `org.nixos.specialisation.v1`.
extern crate alloc;
use crate::{
    json::{self, Json},
    RawEntry,
};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use graph::{BootMethod, EntryRole, OsKind};
use ports::FileStore;

pub const BOOTSPEC_DIR: &str = "\\EFI\\nixos";
pub const V1: &str = "org.nixos.bootspec.v1";
pub const SPEC_V1: &str = "org.nixos.specialisation.v1";

pub fn discover<F: FileStore + ?Sized>(fs: &F) -> Vec<RawEntry> {
    let mut out = Vec::new();
    let names = match fs.list_dir_meta(BOOTSPEC_DIR) {
        Ok(n) => n,
        Err(_) => return out,
    };
    // Only directories can hold a bootspec file. On a systemd-boot-managed NixOS
    // ESP, `\EFI\nixos` is full of kernel/initrd FILES; probing two names under
    // each of those was two failed opens per file, every boot.
    for name in names.into_iter().filter(|m| m.is_dir).map(|m| m.name) {
        // Generation directories are conventionally named with the generation
        // number; we accept any dir that contains a bootspec file.
        for file in ["bootspec.json", "boot.json"] {
            let path = format!("{BOOTSPEC_DIR}\\{name}\\{file}");
            if let Ok(bytes) = fs.read(&path) {
                if let Ok(text) = core::str::from_utf8(&bytes) {
                    if let Ok(doc) = json::parse(text) {
                        collect(&doc, generation_of(&name), &mut out);
                    }
                }
                break;
            }
        }
    }
    out
}

/// Extract the base generation entry and any specialisations from one document.
fn collect(doc: &Json, gen: Option<u32>, out: &mut Vec<RawEntry>) {
    if let Some(base) = doc.path(&[V1]) {
        if let Some(e) = entry_from_bootspec(base, gen, None) {
            out.push(e);
        }
    }
    if let Some(Json::Obj(specs)) = doc.get(SPEC_V1) {
        for (spec_name, spec_doc) in specs {
            if let Some(inner) = spec_doc.path(&[V1]) {
                if let Some(e) = entry_from_bootspec(inner, gen, Some(spec_name.clone())) {
                    out.push(e);
                }
            }
        }
    }
}

fn entry_from_bootspec(bs: &Json, gen: Option<u32>, spec: Option<String>) -> Option<RawEntry> {
    let kernel = bs.get("kernel")?.as_str()?;
    let initrd = bs.get("initrd").and_then(|j| j.as_str());
    let label = bs.get("label").and_then(|j| j.as_str()).unwrap_or("NixOS");

    // kernelParams is an array of strings → a single cmdline.
    let cmdline = bs
        .get("kernelParams")
        .and_then(|j| j.as_array())
        .map(|arr| {
            let mut s = String::new();
            for (i, p) in arr.iter().enumerate() {
                if let Some(ps) = p.as_str() {
                    if i > 0 {
                        s.push(' ');
                    }
                    s.push_str(ps);
                }
            }
            s
        });

    let (role, title) = match (gen, &spec) {
        (Some(g), Some(name)) => (
            EntryRole::Specialisation {
                parent: g,
                name: name.clone(),
            },
            format!("{label} (gen {g}, {name})"),
        ),
        (Some(g), None) => (EntryRole::Generation(g), format!("{label} (gen {g})")),
        (None, Some(name)) => (
            EntryRole::Specialisation {
                parent: 0,
                name: name.clone(),
            },
            format!("{label} ({name})"),
        ),
        (None, None) => (EntryRole::Default, String::from(label)),
    };

    let mut e = RawEntry::new(
        OsKind::NixOs,
        "NixOS",
        title,
        role,
        BootMethod::NixGeneration,
    );
    e.kernel = Some(to_efi(kernel));
    e.initrd = initrd.map(to_efi);
    e.cmdline = cmdline;
    Some(e)
}

/// Parse a generation number out of a directory name, if present
/// (accepts forms like `128`, `nixos-128`, `generation-128`).
fn generation_of(dir: &str) -> Option<u32> {
    let digits: String = dir
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    let forward: String = digits.chars().rev().collect();
    forward.parse::<u32>().ok()
}

fn to_efi(p: &str) -> String {
    let mut s = String::with_capacity(p.len());
    for c in p.chars() {
        s.push(if c == '/' { '\\' } else { c });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use ports::mock::MockFileStore;

    const BS: &str = r#"{ "org.nixos.bootspec.v1": {
        "label": "NixOS 24.05",
        "kernel": "/EFI/nixos/k-128/bzImage",
        "kernelParams": ["init=/nix/init", "loglevel=4"],
        "initrd": "/EFI/nixos/k-128/initrd"
      },
      "org.nixos.specialisation.v1": {
        "gaming": { "org.nixos.bootspec.v1": { "label": "NixOS gaming", "kernel": "/EFI/nixos/k-128/bzImage", "initrd": "/EFI/nixos/k-128/initrd", "kernelParams": [] } }
      } }"#;

    #[test]
    fn discovers_generation_and_specialisation() {
        let fs = MockFileStore::new()
            .with_file("\\EFI\\nixos\\generation-128\\bootspec.json", BS.as_bytes());
        let entries = discover(&fs);
        assert_eq!(entries.len(), 2, "base generation + one specialisation");
        let base = entries
            .iter()
            .find(|e| matches!(e.role, EntryRole::Generation(128)))
            .unwrap();
        assert_eq!(base.kernel.as_deref(), Some("\\EFI\\nixos\\k-128\\bzImage"));
        assert_eq!(base.cmdline.as_deref(), Some("init=/nix/init loglevel=4"));
        assert!(entries.iter().any(|e| matches!(&e.role, EntryRole::Specialisation { parent: 128, name } if name == "gaming")));
    }

    #[test]
    fn generation_number_parsing() {
        assert_eq!(generation_of("generation-128"), Some(128));
        assert_eq!(generation_of("128"), Some(128));
        assert_eq!(generation_of("nixos"), None);
    }

    #[test]
    fn files_in_the_nixos_dir_are_not_probed_as_generations() {
        // Kernel/initrd files sit beside generation dirs; only the dir yields an entry.
        let doc = br#"{"org.nixos.bootspec.v1":{"kernel":"/EFI/nixos/k.efi","label":"NixOS"}}"#;
        let fs = MockFileStore::new()
            .with_file("\\EFI\\nixos\\k.efi", b"MZ")
            .with_file("\\EFI\\nixos\\i.efi", b"x")
            .with_file("\\EFI\\nixos\\7\\boot.json", doc);
        let out = discover(&fs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].role, EntryRole::Generation(7));
    }
}
