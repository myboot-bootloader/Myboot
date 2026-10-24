//! Boot-stage timing. One job: measure and log stage durations. Compiled to a
//! no-op unless the `boot-timing` feature is enabled, so default builds pay
//! nothing (calibration alone costs a 10 ms stall).

#[cfg(feature = "boot-timing")]
mod imp {
    use arch::x86_64::tsc::read_tsc;
    use uefi::boot;

    /// Lap timer over the TSC, calibrated against the firmware's `Stall`.
    pub struct StageClock { ticks_per_us: u64, start: u64, last: u64 }

    impl StageClock {
        /// Calibrate the TSC against a 10 ms firmware stall, then start.
        pub fn start() -> Self {
            let a = read_tsc();
            boot::stall(10_000);
            let b = read_tsc();
            let ticks_per_us = (b.wrapping_sub(a) / 10_000).max(1);
            let now = read_tsc();
            StageClock { ticks_per_us, start: now, last: now }
        }

        /// Log the time since the previous lap and since `start`.
        pub fn lap(&mut self, stage: &str) {
            let now = read_tsc();
            log::info!(
                "myboot: timing {stage} {}us total={}us",
                now.wrapping_sub(self.last) / self.ticks_per_us,
                now.wrapping_sub(self.start) / self.ticks_per_us,
            );
            self.last = now;
        }
    }
}

#[cfg(not(feature = "boot-timing"))]
mod imp {
    /// Feature off: zero-sized, every call compiles away.
    pub struct StageClock;

    impl StageClock {
        pub fn start() -> Self { StageClock }
        pub fn lap(&mut self, _stage: &str) {}
    }
}

pub use imp::StageClock;
