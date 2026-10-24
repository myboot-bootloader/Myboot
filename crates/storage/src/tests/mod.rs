//! Off-metal tests for the parsers (report §5.4). These assert the SAME
//! properties the Kani harnesses prove, but by example — fast feedback on host.
extern crate std;
use crate::bytes;
use crate::fat::FatBpb;
use crate::gpt::{parse_partitions, GptEntry, GptHeader, GPT_SIGNATURE};
use crate::mbr;
use crate::pe::{self, PeHeader};
use alloc::vec;
use alloc::vec::Vec;

// ---- the master safety property: NO input panics any parser ----
#[test]
fn parsers_never_panic_on_adversarial_input() {
    // empty, tiny, all-zero, all-0xFF, and a truncated GPT header
    let cases: Vec<Vec<u8>> = vec![
        vec![],
        vec![0x00],
        vec![0xFF; 3],
        vec![0x00; 512],
        vec![0xFF; 512],
        {
            let mut v = GPT_SIGNATURE.to_vec(); // valid sig, then truncated
            v.extend_from_slice(&[0u8; 10]);
            v
        },
    ];
    for c in &cases {
        let _ = GptHeader::parse(c);
        let _ = GptEntry::parse(c);
        let _ = mbr::is_protective_gpt(c);
        let _ = FatBpb::parse(c);
        let _ = PeHeader::parse(c);
        let _ = pe::section::find(c, b".sbat");
    }
    // exhaustively fuzz every length 0..300 of a repeating pattern
    let base: Vec<u8> = (0..=255u16).map(|x| x as u8).cycle().take(300).collect();
    for len in 0..300 {
        let s = &base[..len];
        let _ = GptHeader::parse(s);
        let _ = GptEntry::parse(s);
        let _ = pe::section::find(s, b".sbat");
        let _ = mbr::is_protective_gpt(s);
        let _ = FatBpb::parse(s);
        let _ = PeHeader::parse(s);
    }
}

#[test]
fn gpt_header_roundtrips_minimal_valid() {
    let mut b = vec![0u8; 92];
    b[..8].copy_from_slice(GPT_SIGNATURE);
    b[72..80].copy_from_slice(&2u64.to_le_bytes()); // entry_lba
    b[80..84].copy_from_slice(&128u32.to_le_bytes()); // num_entries
    b[84..88].copy_from_slice(&128u32.to_le_bytes()); // entry_size
    let h = GptHeader::parse(&b).expect("valid header");
    assert_eq!(h.entry_lba, 2);
    assert_eq!(h.num_entries, 128);
    assert_eq!(h.entry_size, 128);
}

#[test]
fn gpt_header_rejects_bad_signature_and_entry_size() {
    let mut b = vec![0u8; 92];
    b[..8].copy_from_slice(b"NOTAPART");
    assert!(GptHeader::parse(&b).is_err());
    b[..8].copy_from_slice(GPT_SIGNATURE);
    b[84..88].copy_from_slice(&64u32.to_le_bytes()); // entry_size < 128
    assert!(GptHeader::parse(&b).is_err());
}

#[test]
fn gpt_entry_empty_slot_is_none_and_ordering_checked() {
    let empty = vec![0u8; 128];
    assert_eq!(GptEntry::parse(&empty).unwrap(), None);

    // valid entry: nonzero guid, first<=last
    let mut e = vec![0u8; 128];
    e[0] = 0x28;
    e[1] = 0x73; // nonzero type guid start
    e[32..40].copy_from_slice(&100u64.to_le_bytes());
    e[40..48].copy_from_slice(&200u64.to_le_bytes());
    let parsed = GptEntry::parse(&e).unwrap().unwrap();
    assert_eq!(parsed.first_lba, 100);
    assert_eq!(parsed.last_lba, 200);

    // last < first must be rejected (downstream relies on this)
    e[40..48].copy_from_slice(&50u64.to_le_bytes());
    assert!(GptEntry::parse(&e).is_err());
}

#[test]
fn parse_partitions_skips_empty_and_stops_on_short_buffer() {
    let mut b = vec![0u8; 92];
    b[..8].copy_from_slice(GPT_SIGNATURE);
    b[72..80].copy_from_slice(&2u64.to_le_bytes());
    b[80..84].copy_from_slice(&4u32.to_le_bytes()); // claims 4 entries
    b[84..88].copy_from_slice(&128u32.to_le_bytes());
    let h = GptHeader::parse(&b).unwrap();

    // provide only ONE valid entry then run out — must not panic, returns 1
    let mut arr = vec![0u8; 128];
    arr[0] = 0x28;
    arr[1] = 0x73;
    arr[32..40].copy_from_slice(&34u64.to_le_bytes());
    arr[40..48].copy_from_slice(&99u64.to_le_bytes());
    let parts = parse_partitions(&h, &arr).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].start_lba, 34);
}

#[test]
fn mbr_detects_protective_gpt() {
    let mut b = vec![0u8; 512];
    b[510] = 0x55;
    b[511] = 0xAA;
    b[446 + 4] = 0xEE; // first partition type = protective GPT
    assert!(mbr::is_protective_gpt(&b).unwrap());
    b[446 + 4] = 0x83;
    assert!(!mbr::is_protective_gpt(&b).unwrap());
}

#[test]
fn fat_bpb_parses_and_rejects() {
    let mut b = vec![0u8; 512];
    b[510] = 0x55;
    b[511] = 0xAA;
    b[3..11].copy_from_slice(b"MSDOS5.0");
    b[11..13].copy_from_slice(&512u16.to_le_bytes()); // bytes/sector
    b[13] = 8; // sectors/cluster
    b[14..16].copy_from_slice(&32u16.to_le_bytes());
    b[16] = 2;
    let bpb = FatBpb::parse(&b).unwrap();
    assert_eq!(bpb.bytes_per_sector, 512);
    assert_eq!(bpb.sectors_per_cluster, 8);
    assert_eq!(bpb.oem, "MSDOS5.0");
    // bad sector size rejected
    b[11..13].copy_from_slice(&999u16.to_le_bytes());
    assert!(FatBpb::parse(&b).is_err());
}

#[test]
fn pe_recognises_efi_application_and_rejects_garbage() {
    // Build a minimal PE32+ header with subsystem = EFI application (10).
    let mut b = vec![0u8; 0x200];
    b[0] = b'M';
    b[1] = b'Z';
    let pe_off = 0x80usize;
    b[0x3C..0x40].copy_from_slice(&(pe_off as u32).to_le_bytes());
    b[pe_off..pe_off + 4].copy_from_slice(b"PE\0\0");
    let coff = pe_off + 4;
    b[coff..coff + 2].copy_from_slice(&0x8664u16.to_le_bytes()); // machine x64
    let opt = coff + 20;
    b[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes()); // PE32+
    b[opt + 68..opt + 70].copy_from_slice(&10u16.to_le_bytes()); // subsystem
    let h = PeHeader::parse(&b).unwrap();
    assert!(h.is_efi_application);
    assert_eq!(h.machine, 0x8664);

    assert!(PeHeader::parse(b"not a pe").is_err());
}

#[test]
fn bytes_helpers_are_bounds_checked() {
    let b = [1u8, 2, 3];
    assert_eq!(bytes::u8_at(&b, 2).unwrap(), 3);
    assert!(bytes::u8_at(&b, 3).is_err());
    assert!(bytes::u32_le(&b, 0).is_err()); // only 3 bytes
    assert!(bytes::slice_at(&b, 1, 5).is_err());
    assert_eq!(bytes::u16_le(&b, 0).unwrap(), 0x0201);
}

/// A minimal PE32+ image with the given (name, data) sections laid out after
/// the headers; `virtual_size` = data length, raw size padded to 0x20.
fn pe_with_sections(sections: &[(&[u8], &[u8])]) -> alloc::vec::Vec<u8> {
    let pe_off = 0x80usize;
    let coff = pe_off + 4;
    let opt_size = 0xF0usize;
    let table = coff + 20 + opt_size;
    let mut data_at = table + 40 * sections.len();
    let mut b = alloc::vec![0u8; data_at];
    b[0] = b'M';
    b[1] = b'Z';
    b[0x3C..0x40].copy_from_slice(&(pe_off as u32).to_le_bytes());
    b[pe_off..pe_off + 4].copy_from_slice(b"PE\0\0");
    b[coff + 2..coff + 4].copy_from_slice(&(sections.len() as u16).to_le_bytes());
    b[coff + 16..coff + 18].copy_from_slice(&(opt_size as u16).to_le_bytes());
    for (i, (name, data)) in sections.iter().enumerate() {
        let h = table + 40 * i;
        b[h..h + name.len()].copy_from_slice(name);
        let raw = (data.len() + 0x1F) & !0x1F;
        b[h + 8..h + 12].copy_from_slice(&(data.len() as u32).to_le_bytes());
        b[h + 16..h + 20].copy_from_slice(&(raw as u32).to_le_bytes());
        b[h + 20..h + 24].copy_from_slice(&(data_at as u32).to_le_bytes());
        b.extend_from_slice(data);
        b.resize(data_at + raw, 0);
        data_at += raw;
    }
    b
}

#[test]
fn pe_section_find_returns_the_named_section_trimmed_to_virtual_size() {
    let img = pe_with_sections(&[(b".text", b"code"), (b".sbat", b"sbat,1,x\n")]);
    assert_eq!(pe::section::find(&img, b".sbat").unwrap(), b"sbat,1,x\n");
    assert_eq!(pe::section::find(&img, b".text").unwrap(), b"code");
    assert!(pe::section::find(&img, b".data").is_err(), "absent section");
    assert!(
        pe::section::find(&img, b".sba").is_err(),
        "prefix must not match"
    );
}

#[test]
fn pe_section_find_rejects_malformed_images() {
    let img = pe_with_sections(&[(b".sbat", b"sbat,1,x\n")]);
    assert!(pe::section::find(b"not a pe", b".sbat").is_err());
    // Truncated in the middle of the section table / section data.
    let data_at = 0x80 + 4 + 20 + 0xF0 + 40;
    for cut in [0x80, 0x80 + 4 + 20 + 0xF0 + 10, data_at + 4] {
        assert!(
            pe::section::find(&img[..cut], b".sbat").is_err(),
            "cut at {cut}"
        );
    }
    // Only alignment padding cut off: the section's VirtualSize bytes are intact.
    assert_eq!(
        pe::section::find(&img[..img.len() - 1], b".sbat").unwrap(),
        b"sbat,1,x\n"
    );
    // Section count above the PE/COFF limit.
    let mut bad = img.clone();
    let coff = 0x80 + 4;
    bad[coff + 2..coff + 4].copy_from_slice(&(pe::section::MAX_SECTIONS + 1).to_le_bytes());
    assert!(pe::section::find(&bad, b".sbat").is_err());
    // Raw pointer far past the end of the buffer.
    let mut far = img.clone();
    let h = 0x80 + 4 + 20 + 0xF0;
    far[h + 20..h + 24].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(pe::section::find(&far, b".sbat").is_err());
}
