//! bytes — the single, panic-free primitive for reading untrusted buffers
//! (blueprint §1.2, SSOT for byte access). Every parser reads through here, so
//! Kani only has to trust one set of bounds-checked accessors.
#![allow(clippy::result_unit_err)]

/// Short-buffer error. Parsers map this into their own error types.
#[derive(Debug, PartialEq, Eq)]
pub struct OutOfBounds;

#[inline]
pub fn u8_at(buf: &[u8], at: usize) -> Result<u8, OutOfBounds> {
    buf.get(at).copied().ok_or(OutOfBounds)
}

#[inline]
pub fn slice_at(buf: &[u8], at: usize, len: usize) -> Result<&[u8], OutOfBounds> {
    // `at + len` cannot overflow usize for any real buffer, but be defensive:
    let end = at.checked_add(len).ok_or(OutOfBounds)?;
    buf.get(at..end).ok_or(OutOfBounds)
}

#[inline]
pub fn u16_le(buf: &[u8], at: usize) -> Result<u16, OutOfBounds> {
    let b = slice_at(buf, at, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

#[inline]
pub fn u32_le(buf: &[u8], at: usize) -> Result<u32, OutOfBounds> {
    let b = slice_at(buf, at, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[inline]
pub fn u64_le(buf: &[u8], at: usize) -> Result<u64, OutOfBounds> {
    let b = slice_at(buf, at, 8)?;
    Ok(u64::from_le_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}

/// Read a fixed-length ASCII/OEM field, trimming trailing spaces and NULs.
/// Non-UTF8 bytes are dropped rather than panicking.
pub fn ascii_trimmed(
    buf: &[u8],
    at: usize,
    len: usize,
) -> Result<alloc::string::String, OutOfBounds> {
    use alloc::string::String;
    let b = slice_at(buf, at, len)?;
    let mut s = String::new();
    for &c in b {
        if c == 0 {
            break;
        }
        if c.is_ascii_graphic() || c == b' ' {
            s.push(c as char);
        }
    }
    while s.ends_with(' ') {
        s.pop();
    }
    Ok(s)
}
