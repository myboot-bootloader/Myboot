//! x86_64 architecture support.
pub mod cpu;          // inline asm wrappers (core::arch::asm!)
pub mod boot_params;  // Linux x86 boot-protocol data structures (host-tested)
pub mod handoff;      // Rust side of the direct-kernel handoff
pub mod tsc;          // RDTSC for boot-stage timing (engine `boot-timing` feature)

// Pull in the hand-written assembly (the jump-to-kernel trampoline).
core::arch::global_asm!(include_str!("handoff.s"), options(att_syntax));
