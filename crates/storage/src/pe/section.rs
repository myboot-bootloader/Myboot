//! PE/COFF section lookup by name (PE/COFF spec: section table). One job: find
//! a named section's raw bytes in an image file, e.g. the `.sbat` metadata that
//! shim and GRUB embed. TOTAL: every input yields Ok/Err, never a panic; all
//! access goes through the checked `bytes` helpers.
use crate::bytes;
use super::PeError;

/// PE/COFF limit on the number of sections.
pub const MAX_SECTIONS: u16 = 96;
/// Size of one section-table header.
const SECTION_HEADER: usize = 40;

/// Return the file bytes of the first section named `name` (at most 8 bytes,
/// compared against the NUL-padded header name). The length is `VirtualSize`
/// when it is non-zero and smaller than `SizeOfRawData` (the raw size is padded
/// to the file alignment), else `SizeOfRawData`. `Err` = not a PE image, a
/// malformed table, or no such section.
pub fn find<'a>(buf: &'a [u8], name: &[u8]) -> Result<&'a [u8], PeError> {
    if name.is_empty() || name.len() > 8 { return Err(PeError); }
    if bytes::slice_at(buf, 0, 2)? != b"MZ" { return Err(PeError); }
    let pe_off = bytes::u32_le(buf, 0x3C)? as usize;
    if bytes::slice_at(buf, pe_off, 4)? != b"PE\0\0" { return Err(PeError); }
    // COFF header: NumberOfSections at +2, SizeOfOptionalHeader at +16.
    let coff = pe_off.checked_add(4).ok_or(PeError)?;
    let count = bytes::u16_le(buf, coff.checked_add(2).ok_or(PeError)?)?;
    if count > MAX_SECTIONS { return Err(PeError); }
    let opt_size = bytes::u16_le(buf, coff.checked_add(16).ok_or(PeError)?)? as usize;
    // The section table follows the 20-byte COFF header and the optional header.
    let table = coff.checked_add(20).and_then(|t| t.checked_add(opt_size)).ok_or(PeError)?;
    for i in 0..count as usize {
        let hdr = i.checked_mul(SECTION_HEADER).and_then(|o| table.checked_add(o)).ok_or(PeError)?;
        let raw_name = bytes::slice_at(buf, hdr, 8)?;
        let end = raw_name.iter().position(|&b| b == 0).unwrap_or(8);
        if raw_name.get(..end) != Some(name) { continue; }
        let virtual_size = bytes::u32_le(buf, hdr.checked_add(8).ok_or(PeError)?)? as usize;
        let raw_size = bytes::u32_le(buf, hdr.checked_add(16).ok_or(PeError)?)? as usize;
        let raw_ptr = bytes::u32_le(buf, hdr.checked_add(20).ok_or(PeError)?)? as usize;
        let len = if virtual_size != 0 && virtual_size < raw_size { virtual_size } else { raw_size };
        return Ok(bytes::slice_at(buf, raw_ptr, len)?);
    }
    Err(PeError)
}
