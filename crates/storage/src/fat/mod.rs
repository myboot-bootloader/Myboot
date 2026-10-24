//! FAT boot-sector parsing for the ESP (report §4.6). Total & panic-free.
//! We only need enough to confirm a FAT volume and locate its layout; the full
//! directory walk is a later milestone.
use crate::bytes::{self, OutOfBounds};
use alloc::string::String;
extern crate alloc;

#[derive(Debug, PartialEq, Eq)]
pub struct FatError;
impl From<OutOfBounds> for FatError {
    fn from(_: OutOfBounds) -> Self {
        FatError
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FatBpb {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub num_fats: u8,
    pub oem: String,
}

impl FatBpb {
    /// Parse the BIOS Parameter Block from a FAT boot sector.
    pub fn parse(buf: &[u8]) -> Result<FatBpb, FatError> {
        // 0x55AA boot signature guards obviously-nonsense input.
        if bytes::u8_at(buf, 510)? != 0x55 || bytes::u8_at(buf, 511)? != 0xAA {
            return Err(FatError);
        }
        let bytes_per_sector = bytes::u16_le(buf, 11)?;
        let sectors_per_cluster = bytes::u8_at(buf, 13)?;
        let reserved_sectors = bytes::u16_le(buf, 14)?;
        let num_fats = bytes::u8_at(buf, 16)?;
        let oem = bytes::ascii_trimmed(buf, 3, 8)?;
        // Sanity: a valid sector size is a power of two in [512, 4096].
        if !matches!(bytes_per_sector, 512 | 1024 | 2048 | 4096) {
            return Err(FatError);
        }
        if sectors_per_cluster == 0 {
            return Err(FatError);
        }
        Ok(FatBpb {
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            num_fats,
            oem,
        })
    }
}
