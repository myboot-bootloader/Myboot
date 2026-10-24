//! Protective / legacy MBR parsing (report §4.6). Total & panic-free.
use crate::bytes::{self, OutOfBounds};

#[derive(Debug, PartialEq, Eq)]
pub struct MbrError;
impl From<OutOfBounds> for MbrError {
    fn from(_: OutOfBounds) -> Self {
        MbrError
    }
}

pub const MBR_SIG_OFFSET: usize = 510;
pub const PROTECTIVE_GPT_TYPE: u8 = 0xEE;

/// True if LBA0 is a GPT protective MBR (has a 0xEE partition + 0x55AA signature).
pub fn is_protective_gpt(buf: &[u8]) -> Result<bool, MbrError> {
    if bytes::u8_at(buf, MBR_SIG_OFFSET)? != 0x55 {
        return Ok(false);
    }
    if bytes::u8_at(buf, MBR_SIG_OFFSET + 1)? != 0xAA {
        return Ok(false);
    }
    // Four 16-byte partition records start at 446; type byte is at +4.
    for i in 0..4 {
        let ptype = bytes::u8_at(buf, 446 + i * 16 + 4)?;
        if ptype == PROTECTIVE_GPT_TYPE {
            return Ok(true);
        }
    }
    Ok(false)
}
