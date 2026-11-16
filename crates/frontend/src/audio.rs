//! A shared frame grid for audio sources and output scheduling.
use relocade_time::{TickRate, Timestamp};
use std::time::Duration;

/// Frame zero begins at `origin`; every source uses the same output sample rate.
/// Timestamps stay in native ticks. Only wakeup deadlines round to whole ticks;
/// signal changes retain their position within an output frame.
#[derive(Clone, Copy, Debug)]
pub struct AudioTiming {
    origin: Timestamp,
    tick_rate: TickRate,
    sample_rate: u32,
}

impl AudioTiming {
    pub fn new(origin: Timestamp, tick_rate: TickRate, sample_rate: u32) -> Result<Self, String> {
        if sample_rate == 0 || sample_rate > 1_000_000_000 {
            return Err("audio sample rate must be in 1..=1,000,000,000".into());
        }
        Ok(Self {
            origin,
            tick_rate,
            sample_rate,
        })
    }

    pub fn origin(self) -> Timestamp {
        self.origin
    }
    pub fn tick_rate(self) -> TickRate {
        self.tick_rate
    }
    pub fn sample_rate(self) -> u32 {
        self.sample_rate
    }

    /// Earliest native reading at or after the beginning of `frame`.
    /// Compute from the absolute frame index to avoid accumulating rounding.
    pub fn deadline_for_frame(self, frame: u64) -> Timestamp {
        let ticks = (u128::from(frame) * u128::from(self.tick_rate.ticks_per_second().get()))
            .div_ceil(u128::from(self.sample_rate));
        assert!(
            ticks <= i64::MAX as u128,
            "audio deadline exceeds the clock horizon"
        );
        Timestamp::from_ticks(self.origin.ticks().wrapping_add(ticks as u64))
    }

    /// Position of a signal change within `frame`, from 0.0 to less than 1.0.
    /// Earlier changes return zero; changes at or after the frame end return None.
    /// `delay` locates a device-scheduled change relative to its native anchor,
    /// without rounding each device period to a native tick.
    pub fn event_offset(self, frame: u64, at: Timestamp, delay: Duration) -> Option<f64> {
        let rate = i128::from(self.tick_rate.ticks_per_second().get());
        let samples = i128::from(self.sample_rate);
        let native = i128::from(at.ticks().wrapping_sub(self.origin.ticks()) as i64);
        // One native tick is sample_rate units and one frame is tick_rate units.
        // Split the delay before multiplying by the native rate so long runs
        // never require an absolute floating-point time or a three-rate product.
        let delayed = delay
            .as_nanos()
            .checked_mul(samples as u128)
            .expect("audio delay fits");
        let whole = i128::try_from(delayed / 1_000_000_000).expect("audio delay fits");
        let tail = (delayed % 1_000_000_000) as i128 * rate;
        let position = native * samples
            + whole.checked_mul(rate).expect("audio delay fits")
            + tail / 1_000_000_000;
        let start = i128::from(frame)
            .checked_mul(rate)
            .expect("audio frame fits");
        let offset = position - start;
        if offset >= rate {
            return None;
        }
        if offset < 0 {
            return Some(0.0);
        }
        Some((offset as f64 + (tail % 1_000_000_000) as f64 / 1_000_000_000.0) / rate as f64)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    use super::*;
    use std::num::NonZeroU64;

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn frames_and_delayed_changes_keep_phase_below_one_native_tick() {
        let timing = AudioTiming::new(
            Timestamp::from_ticks(u64::MAX),
            TickRate::new(NonZeroU64::new(1000).unwrap()),
            4000,
        )
        .unwrap();
        assert_eq!(timing.deadline_for_frame(1).ticks(), 0);
        assert_eq!(timing.deadline_for_frame(4).ticks(), 0);
        assert_eq!(timing.deadline_for_frame(5).ticks(), 1);
        let at = timing.origin();
        assert_eq!(
            timing.event_offset(0, at, Duration::from_micros(125)),
            Some(0.5)
        );
        assert_eq!(timing.event_offset(0, at, Duration::from_micros(250)), None);
        assert_eq!(
            timing.event_offset(1, at, Duration::from_micros(250)),
            Some(0.0)
        );
        assert_eq!(
            timing.event_offset(3, Timestamp::from_ticks(0), Duration::ZERO),
            None
        );
        assert_eq!(
            timing.event_offset(4, Timestamp::from_ticks(0), Duration::ZERO),
            Some(0.0)
        );
        // Retain local precision after many frames; only the final division uses f64.
        assert_eq!(
            timing.event_offset(
                4_000_000_001,
                at,
                Duration::from_secs(1_000_000) + Duration::from_micros(375)
            ),
            Some(0.5)
        );
    }
}
