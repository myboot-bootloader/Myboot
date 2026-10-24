# Kani proof harnesses

The `#[kani::proof]` harnesses live in `crates/storage/src/proofs.rs` (report §3.12, §5.4).
Run with:

```
cargo kani -p storage
```

Proves each parser is total (no panic / no out-of-bounds) up to its input bound:
`gpt_header_parse_never_panics`, `gpt_entry_offsets_ordered`, `bls_entry_fields_within_input`, `pe_header_parse_is_total`, `pe_section_find_is_total`.
