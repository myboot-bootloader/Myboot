//! GPT parsing (report §4.6). Total & panic-free; the primary Kani target.
extern crate alloc;
use crate::bytes::{self, OutOfBounds};
use crate::{PartKind, Partition};
use alloc::vec::Vec;

pub const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
pub const SECTOR: usize = 512;

/// Well-known partition-type GUIDs (mixed-endian on disk, compared as raw bytes).
const GUID_EFI_SYSTEM: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];
const GUID_LINUX_FS: [u8; 16] = [
    0xAF, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47, 0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D, 0xE4,
];

#[derive(Debug, PartialEq, Eq)]
pub enum GptError {
    Short,
    BadSignature,
    BadEntrySize,
    TooManyEntries,
}
impl From<OutOfBounds> for GptError {
    fn from(_: OutOfBounds) -> Self {
        GptError::Short
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptHeader {
    pub current_lba: u64,
    pub backup_lba: u64,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub entry_lba: u64,
    pub num_entries: u32,
    pub entry_size: u32,
}

impl GptHeader {
    /// Parse the LBA-1 GPT header from a bounds-checked slice.
    /// TOTAL: every input yields Ok/Err, never a panic (Kani-verified).
    pub fn parse(buf: &[u8]) -> Result<GptHeader, GptError> {
        let sig = bytes::slice_at(buf, 0, 8)?;
        if sig != GPT_SIGNATURE {
            return Err(GptError::BadSignature);
        }
        let current_lba = bytes::u64_le(buf, 24)?;
        let backup_lba = bytes::u64_le(buf, 32)?;
        let first_usable_lba = bytes::u64_le(buf, 40)?;
        let last_usable_lba = bytes::u64_le(buf, 48)?;
        let entry_lba = bytes::u64_le(buf, 72)?;
        let num_entries = bytes::u32_le(buf, 80)?;
        let entry_size = bytes::u32_le(buf, 84)?;
        if entry_size < 128 {
            return Err(GptError::BadEntrySize);
        }
        // Guard against absurd counts that could inflate later allocation.
        if num_entries > 512 {
            return Err(GptError::TooManyEntries);
        }
        Ok(GptHeader {
            current_lba,
            backup_lba,
            first_usable_lba,
            last_usable_lba,
            entry_lba,
            num_entries,
            entry_size,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptEntry {
    pub type_guid: [u8; 16],
    pub first_lba: u64,
    pub last_lba: u64,
}

impl GptEntry {
    /// Parse one partition entry from a 128-byte-aligned record.
    /// Returns Ok(None) for an empty (all-zero type GUID) slot.
    pub fn parse(buf: &[u8]) -> Result<Option<GptEntry>, GptError> {
        let guid = bytes::slice_at(buf, 0, 16)?;
        if guid.iter().all(|&b| b == 0) {
            return Ok(None);
        }
        let mut type_guid = [0u8; 16];
        type_guid.copy_from_slice(guid);
        let first_lba = bytes::u64_le(buf, 32)?;
        let last_lba = bytes::u64_le(buf, 40)?;
        // Structural invariant downstream code relies on (checked by Kani):
        if last_lba < first_lba {
            return Err(GptError::BadEntrySize);
        }
        Ok(Some(GptEntry {
            type_guid,
            first_lba,
            last_lba,
        }))
    }

    pub fn kind(&self) -> PartKind {
        if self.type_guid == GUID_EFI_SYSTEM {
            PartKind::EfiSystem
        } else if self.type_guid == GUID_LINUX_FS {
            PartKind::Linux
        } else {
            PartKind::Unknown
        }
    }
}

/// Parse the header + the entry array (`entries_buf` starting at `entry_lba`).
/// Never panics for any inputs; skips empty slots; stops at `num_entries`.
pub fn parse_partitions(
    header: &GptHeader,
    entries_buf: &[u8],
) -> Result<Vec<Partition>, GptError> {
    let mut out = Vec::new();
    let step = header.entry_size as usize;
    for i in 0..(header.num_entries as usize) {
        let at = i.checked_mul(step).ok_or(GptError::Short)?;
        // If the buffer is short, stop cleanly rather than erroring the whole disk.
        let rec = match bytes::slice_at(entries_buf, at, step) {
            Ok(r) => r,
            Err(_) => break,
        };
        if let Some(e) = GptEntry::parse(rec)? {
            out.push(Partition {
                start_lba: e.first_lba,
                end_lba: e.last_lba,
                kind: e.kind(),
            });
        }
    }
    Ok(out)
}
