//! EFI_LOAD_OPTION encode/decode (report §3.9, verified facts). A `Boot####`
//! variable is: Attributes(u32 LE) · FilePathListLength(u16 LE) · Description
//! (CHAR16, NUL-terminated) · FilePathList(bytes) · OptionalData(bytes).
//!
//! Pure and panic-free: encoding never overflows; decoding is total over `&[u8]`.
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

/// LOAD_OPTION_ACTIVE — the entry is eligible to be booted.
pub const LOAD_OPTION_ACTIVE: u32 = 0x0000_0001;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadOption {
    pub attributes: u32,
    pub description: String,
    /// The device-path bytes exactly as they must appear on the medium. MyBoot
    /// treats these opaquely (it does not synthesise device paths itself yet).
    pub file_path_list: Vec<u8>,
    pub optional_data: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LoadOptError {
    Short,
    BadDescription,
    LengthOverflow,
}

impl LoadOption {
    pub fn new(description: impl Into<String>, file_path_list: Vec<u8>) -> Self {
        LoadOption {
            attributes: LOAD_OPTION_ACTIVE,
            description: description.into(),
            file_path_list,
            optional_data: Vec::new(),
        }
    }

    /// Serialise to the exact `Boot####` byte layout. Total — the only failure
    /// is a file-path list longer than u16::MAX (a firmware limit), reported not
    /// panicked.
    pub fn encode(&self) -> Result<Vec<u8>, LoadOptError> {
        let fpl_len: u16 =
            u16::try_from(self.file_path_list.len()).map_err(|_| LoadOptError::LengthOverflow)?;
        let mut out = Vec::with_capacity(
            4 + 2
                + self.description.len() * 2
                + 2
                + self.file_path_list.len()
                + self.optional_data.len(),
        );
        out.extend_from_slice(&self.attributes.to_le_bytes());
        out.extend_from_slice(&fpl_len.to_le_bytes());
        // Description as UCS-2/UTF-16LE, NUL-terminated.
        for u in self.description.encode_utf16() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0]); // CHAR16 NUL terminator
        out.extend_from_slice(&self.file_path_list);
        out.extend_from_slice(&self.optional_data);
        Ok(out)
    }

    /// Parse a `Boot####` value. Total over any `&[u8]`; never panics.
    pub fn decode(buf: &[u8]) -> Result<LoadOption, LoadOptError> {
        let attr_b = buf.get(0..4).ok_or(LoadOptError::Short)?;
        let attributes = u32::from_le_bytes([attr_b[0], attr_b[1], attr_b[2], attr_b[3]]);
        let len_b = buf.get(4..6).ok_or(LoadOptError::Short)?;
        let fpl_len = u16::from_le_bytes([len_b[0], len_b[1]]) as usize;

        // Description: UTF-16LE units until a 0x0000 unit.
        let mut i = 6usize;
        let mut units: Vec<u16> = Vec::new();
        loop {
            let u_b = buf.get(i..i + 2).ok_or(LoadOptError::BadDescription)?;
            let u = u16::from_le_bytes([u_b[0], u_b[1]]);
            i += 2;
            if u == 0 {
                break;
            }
            units.push(u);
        }
        let description = String::from_utf16(&units).map_err(|_| LoadOptError::BadDescription)?;

        let fpl_end = i.checked_add(fpl_len).ok_or(LoadOptError::LengthOverflow)?;
        let file_path_list = buf.get(i..fpl_end).ok_or(LoadOptError::Short)?.to_vec();
        let optional_data = buf.get(fpl_end..).unwrap_or(&[]).to_vec();

        Ok(LoadOption {
            attributes,
            description,
            file_path_list,
            optional_data,
        })
    }

    pub fn is_active(&self) -> bool {
        self.attributes & LOAD_OPTION_ACTIVE != 0
    }
}

/// Encode a `BootOrder` variable (array of u16 LE boot-option numbers).
pub fn encode_boot_order(order: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(order.len() * 2);
    for n in order {
        out.extend_from_slice(&n.to_le_bytes());
    }
    out
}

/// Decode a `BootOrder` variable. Total; a trailing odd byte is ignored.
pub fn decode_boot_order(buf: &[u8]) -> Vec<u16> {
    // `as_chunks` yields only whole 2-byte chunks; the odd tail is the remainder.
    buf.as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn load_option_roundtrips() {
        let lo = LoadOption::new("Windows Boot Manager", vec![1, 2, 3, 4]);
        let bytes = lo.encode().unwrap();
        let back = LoadOption::decode(&bytes).unwrap();
        assert_eq!(back, lo);
        assert!(back.is_active());
    }

    #[test]
    fn load_option_decode_is_total_on_garbage() {
        for len in 0..40usize {
            let junk: Vec<u8> = (0..len).map(|x| x as u8).collect();
            let _ = LoadOption::decode(&junk); // must never panic
        }
        assert!(LoadOption::decode(&[]).is_err());
    }

    #[test]
    fn boot_order_roundtrips_and_tolerates_odd_tail() {
        let order = [0x0000u16, 0x0001, 0x0007];
        let enc = encode_boot_order(&order);
        assert_eq!(decode_boot_order(&enc), order.to_vec());
        let mut odd = enc.clone();
        odd.push(0xAB); // stray byte
        assert_eq!(decode_boot_order(&odd), order.to_vec());
    }
}
