//! Discover existing UEFI Boot#### options from firmware variables — the same
//! information `efibootmgr` shows (report verified facts). Each active option
//! with a recognisable description becomes a generic chainload `RawEntry`.
extern crate alloc;
use crate::RawEntry;
use alloc::vec::Vec;
use graph::{BootMethod, EntryRole, OsKind};
use persistence::loadopt::LoadOption;
use ports::{VarNamespace, VarStore};

/// Return one RawEntry per active Boot#### variable we can decode. We classify by
/// the description text (Windows Boot Manager, etc.); anything else is GenericEfi.
pub fn discover<V: VarStore>(vars: &V) -> Vec<RawEntry> {
    let mut out = Vec::new();
    let names = match vars.list(VarNamespace::Global) {
        Ok(n) => n,
        Err(_) => return out,
    };
    for name in names {
        // Boot#### where #### are four uppercase hex digits.
        if !is_boot_option_name(&name) {
            continue;
        }
        let bytes = match vars.get(VarNamespace::Global, &name) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let opt = match LoadOption::decode(&bytes) {
            Ok(o) => o,
            Err(_) => continue,
        };
        if !opt.is_active() {
            continue;
        }
        let (kind, os_name) = classify(&opt.description);
        // Firmware options carry a device path, not a clean loader path. For a
        // well-known OS we attach its canonical loader so this entry shares a
        // stable id with the same OS discovered by the ESP probe (they merge).
        let mut e = RawEntry::new(
            kind,
            os_name,
            opt.description.clone(),
            EntryRole::Default,
            chain_method(kind),
        );
        e.loader = canonical_loader(kind);
        out.push(e);
    }
    out
}

fn is_boot_option_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 8 && &b[..4] == b"Boot" && b[4..].iter().all(|c| c.is_ascii_hexdigit())
}

fn classify(desc: &str) -> (OsKind, &'static str) {
    let d = desc.to_ascii_lowercase();
    if d.contains("windows") {
        (OsKind::Windows, "Windows")
    } else if d.contains("nixos") {
        (OsKind::NixOs, "NixOS")
    } else if d.contains("linux") || d.contains("ubuntu") || d.contains("fedora") {
        (OsKind::Linux, "Linux")
    } else {
        (OsKind::GenericEfi, "EFI")
    }
}

fn canonical_loader(kind: OsKind) -> Option<alloc::string::String> {
    match kind {
        // Verified fact: the Windows loader is bootmgfw.efi.
        OsKind::Windows => Some(crate::esp::WINDOWS_LOADER.into()),
        _ => None, // other firmware options only expose an opaque device path
    }
}

fn chain_method(kind: OsKind) -> BootMethod {
    match kind {
        OsKind::Windows => BootMethod::WindowsBootManager,
        _ => BootMethod::EfiChainload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use persistence::loadopt::LoadOption;
    use ports::mock::MockVarStore;

    fn put_opt(vs: &mut MockVarStore, name: &str, desc: &str) {
        let bytes = LoadOption::new(desc, alloc::vec![1, 2, 3, 4])
            .encode()
            .unwrap();
        vs.set(VarNamespace::Global, name, &bytes).unwrap();
    }

    #[test]
    fn decodes_and_classifies_boot_options() {
        let mut vs = MockVarStore::new();
        put_opt(&mut vs, "Boot0000", "Windows Boot Manager");
        put_opt(&mut vs, "Boot0001", "ubuntu");
        vs.set(VarNamespace::Global, "BootOrder", &[0, 0]).unwrap(); // ignored (not Boot####)
        let entries = discover(&vs);
        assert_eq!(entries.len(), 2);
        assert!(entries
            .iter()
            .any(|e| e.kind == OsKind::Windows && e.method == BootMethod::WindowsBootManager));
        assert!(entries.iter().any(|e| e.kind == OsKind::Linux));
    }

    #[test]
    fn ignores_non_boot_variables_and_bad_data() {
        let mut vs = MockVarStore::new();
        vs.set(VarNamespace::Global, "BootCurrent", &[0, 0])
            .unwrap();
        vs.set(VarNamespace::Global, "Boot0002", &[0xFF; 3])
            .unwrap(); // undecodable
        assert!(discover(&vs).is_empty());
    }
}
