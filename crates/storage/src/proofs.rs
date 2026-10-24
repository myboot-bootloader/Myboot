//! Kani proof harnesses (report §3.12). Run with `cargo kani -p storage`.
//! Each harness proves a parser is TOTAL — no panic, overflow, or OOB — for ALL
//! inputs up to the given bound. Compiled only under `--cfg kani`.
use crate::fat::FatBpb;
use crate::gpt::{parse_partitions, GptEntry, GptHeader};
use crate::mbr;
use crate::pe::section;
use crate::pe::PeHeader;

#[kani::proof]
fn gpt_header_parse_never_panics() {
    let buf: [u8; 96] = kani::any();
    let _ = GptHeader::parse(&buf);
}

#[kani::proof]
fn gpt_entry_offsets_ordered() {
    let buf: [u8; 128] = kani::any();
    if let Ok(Some(e)) = GptEntry::parse(&buf) {
        assert!(e.first_lba <= e.last_lba);
    }
}

#[kani::proof]
#[kani::unwind(6)]
fn parse_partitions_never_panics() {
    // small symbolic header + a bounded entry buffer
    let hbuf: [u8; 96] = kani::any();
    if let Ok(h) = GptHeader::parse(&hbuf) {
        let arr: [u8; 512] = kani::any();
        let _ = parse_partitions(&h, &arr);
    }
}

#[kani::proof]
fn mbr_is_total() {
    let buf: [u8; 512] = kani::any();
    let _ = mbr::is_protective_gpt(&buf);
}

#[kani::proof]
fn fat_bpb_is_total() {
    let buf: [u8; 512] = kani::any();
    let _ = FatBpb::parse(&buf);
}

#[kani::proof]
fn pe_header_is_total() {
    let buf: [u8; 128] = kani::any();
    let _ = PeHeader::parse(&buf);
}

#[kani::proof]
#[kani::unwind(4)]
fn pe_section_find_is_total() {
    let buf: [u8; 256] = kani::any();
    let _ = section::find(&buf, b".sbat");
}
