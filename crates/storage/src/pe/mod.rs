//! PE/COFF recognition of EFI images (report §4.6). Total & panic-free.
//! We validate just enough to accept/reject an image for chainloading; we do NOT
//! load or relocate it (the firmware LoadImage does that).
use crate::bytes::{self, OutOfBounds};

pub mod section;

#[derive(Debug, PartialEq, Eq)]
pub struct PeError;
impl From<OutOfBounds> for PeError {
    fn from(_: OutOfBounds) -> Self {
        PeError
    }
}

/// UEFI application PE subsystem value (IMAGE_SUBSYSTEM_EFI_APPLICATION).
pub const SUBSYSTEM_EFI_APPLICATION: u16 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeHeader {
    pub machine: u16,
    pub subsystem: u16,
    pub is_efi_application: bool,
}

impl PeHeader {
    /// Parse a PE image header. Rejects truncated/malformed images with Err,
    /// never a slice panic.
    pub fn parse(buf: &[u8]) -> Result<PeHeader, PeError> {
        // DOS header: 'MZ' magic, e_lfanew (u32) at 0x3C points to the PE header.
        if bytes::slice_at(buf, 0, 2)? != b"MZ" {
            return Err(PeError);
        }
        let pe_off = bytes::u32_le(buf, 0x3C)? as usize;
        if bytes::slice_at(buf, pe_off, 4)? != b"PE\0\0" {
            return Err(PeError);
        }
        // COFF file header follows the 4-byte PE signature.
        let coff = pe_off.checked_add(4).ok_or(PeError)?;
        let machine = bytes::u16_le(buf, coff)?;
        // Optional header starts after the 20-byte COFF header; magic tells 32/64.
        let opt = coff.checked_add(20).ok_or(PeError)?;
        let magic = bytes::u16_le(buf, opt)?;
        // Subsystem field offset differs by PE32 (0x10B) vs PE32+ (0x20B).
        let subsystem_off = match magic {
            0x10B => opt.checked_add(68).ok_or(PeError)?, // PE32
            0x20B => opt.checked_add(68).ok_or(PeError)?, // PE32+ (same relative offset)
            _ => return Err(PeError),
        };
        let subsystem = bytes::u16_le(buf, subsystem_off)?;
        Ok(PeHeader {
            machine,
            subsystem,
            is_efi_application: subsystem == SUBSYSTEM_EFI_APPLICATION,
        })
    }
}
