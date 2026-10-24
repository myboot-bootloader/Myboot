//! Time-stamp counter read for boot-stage timing. One job: return RDTSC.

/// Current TSC value. Invariant-TSC CPUs (every x86_64 CPU MyBoot targets in
/// practice) tick at a constant rate, so deltas convert to wall time once
/// calibrated against a known delay (see `engine::timing`).
#[inline]
pub fn read_tsc() -> u64 {
    // SAFETY: RDTSC exists on every x86_64 CPU and has no memory effects.
    #[allow(unused_unsafe)]
    unsafe { core::arch::x86_64::_rdtsc() }
}
