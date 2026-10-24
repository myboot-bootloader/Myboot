//! SBAT metadata → distro name. One job: read the `.sbat` section text that
//! shim, GRUB and systemd-boot embed for Secure Boot revocation and say which
//! distro built the binary. Each line is CSV:
//! `component_name,generation,vendor_name,package_name,version,url`. Vendor-
//! specific components are named `<component>.<vendor>` (e.g. `grub.ubuntu`) and
//! appear AFTER their upstream ones, so the last known vendor is the most
//! downstream: Parrot's GRUB lists `grub.debian`, `grub.debian14`, `grub.parrot`.
//!
//! Matching is EXACT against the distro table, never a substring search over the
//! binary, so unrelated text (e.g. "details" containing "tails") cannot match.
//! Total: malformed input yields `None`.
extern crate alloc;
use crate::esp::DISTRO_MARKERS;
use alloc::string::String;

/// The distro named by the last `<component>.<vendor>` entry whose vendor is a
/// known distro id, or `None`. Non-distro vendor suffixes (e.g. `grub.peimage`)
/// and unknown ones (e.g. `grub.debian14`) are skipped.
pub fn distro_from_sbat(section: &[u8]) -> Option<String> {
    // The section is NUL-padded to its virtual size.
    let end = section
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(section.len());
    let text = core::str::from_utf8(section.get(..end)?).ok()?;
    let mut found: Option<&'static str> = None;
    for line in text.lines() {
        let component = line.split(',').next().unwrap_or("").trim();
        let vendor = match component.rsplit_once('.') {
            Some((_, v)) => v,
            None => continue,
        };
        if let Some((_, name)) = DISTRO_MARKERS
            .iter()
            .find(|(id, _)| id.eq_ignore_ascii_case(vendor))
        {
            found = Some(name);
        }
    }
    found.map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARROT_GRUB: &[u8] =
        b"sbat,1,SBAT Version,sbat,1,https://github.com/rhboot/shim/blob/main/SBAT.md\n\
grub,5,Free Software Foundation,grub,2.14,https://www.gnu.org/software/grub/\n\
grub.debian,5,Debian,grub2,2.14-2parrot3,https://tracker.debian.org/pkg/grub2\n\
grub.debian14,1,Debian,grub2,2.14-2parrot3,https://tracker.debian.org/pkg/grub2\n\
grub.parrot,1,Parrot Security,grub2,2.14-2parrot3,https://gitlab.com/parrotsec/packages/grub2\n";

    #[test]
    fn last_known_vendor_wins() {
        assert_eq!(distro_from_sbat(PARROT_GRUB).as_deref(), Some("Parrot OS"));
    }

    #[test]
    fn non_distro_vendor_suffixes_are_skipped() {
        let ubuntu = b"sbat,1,SBAT Version,sbat,1,x\n\
grub.ubuntu,2,Ubuntu,grub2,2.14-2ubuntu1,https://www.ubuntu.com/\n\
grub.peimage,2,Canonical,grub2,2.14-2ubuntu1,https://salsa.debian.org/\n";
        assert_eq!(distro_from_sbat(ubuntu).as_deref(), Some("Ubuntu"));
    }

    #[test]
    fn debian_signed_shim_names_debian() {
        let shim = b"sbat,1,SBAT Version,sbat,1,x\nshim,4,UEFI shim,shim,1,x\n\
shim.debian,1,Debian,shim,15.8,https://tracker.debian.org/pkg/shim\n";
        assert_eq!(distro_from_sbat(shim).as_deref(), Some("Debian"));
    }

    #[test]
    fn nul_padding_is_ignored_and_upstream_only_is_none() {
        let mut padded = b"sbat,1,x\ngrub,5,Free Software Foundation,grub,2.14,x\n".to_vec();
        padded.extend_from_slice(&[0u8; 32]);
        assert_eq!(distro_from_sbat(&padded), None, "no vendor-specific entry");
    }

    #[test]
    fn malformed_input_is_none() {
        assert_eq!(distro_from_sbat(b""), None);
        assert_eq!(distro_from_sbat(&[0xFF, 0xFE, b'.', b'u']), None);
        assert_eq!(distro_from_sbat(b"no commas or dots at all"), None);
        assert_eq!(
            distro_from_sbat(b"details,1,x\n"),
            None,
            "no substring matching"
        );
    }
}
