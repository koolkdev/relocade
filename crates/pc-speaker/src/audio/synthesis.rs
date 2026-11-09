//! Integrates timestamped speaker changes into complete PCM sample intervals.

use relocade_machine::Instant;
use relocade_pit::{Channel2State, INPUT_HZ};
use std::{collections::VecDeque, time::Duration};

pub(super) struct Signal {
    pub(super) state: Channel2State,
    pub(super) enabled: bool,
}

impl Signal {
    /// High time in billionths of one PIT input clock. Advancing while muted
    /// preserves phase; only the audible contribution becomes zero.
    fn advance_to(&mut self, end: Instant) -> u128 {
        let span = end.duration_since(self.state.at).as_nanos() * u128::from(INPUT_HZ)
            + u128::from(self.state.fraction);
        let before = u128::from(self.state.counter.output());
        let high = self.state.counter.advance((span / 1_000_000_000) as u64);
        let fraction = (span % 1_000_000_000) as u32;
        let integral = u128::from(high) * 1_000_000_000
            + u128::from(fraction) * u128::from(self.state.counter.output())
            - u128::from(self.state.fraction) * before;
        self.state.fraction = fraction;
        self.state.at = end;
        if self.enabled {
            integral
        } else {
            0
        }
    }
}

pub(super) struct Synthesis {
    rate: u64,
    origin: Instant,
    frames: u64,
    signal: Signal,
}

impl Synthesis {
    pub(super) fn new(rate: u32, signal: Signal) -> Self {
        Self {
            rate: u64::from(rate),
            origin: signal.state.at,
            frames: 0,
            signal,
        }
    }

    fn frame_time(&self, frame: u64) -> Instant {
        self.origin
            + Duration::new(
                frame / self.rate,
                ((frame % self.rate) * 1_000_000_000).div_ceil(self.rate) as u32,
            )
    }

    pub(super) fn deadline(&self, frames: usize) -> Instant {
        self.frame_time(self.frames + frames as u64)
    }

    /// Only call once the device clock has reached the entire buffer's deadline.
    /// Every earlier write then exists in the execution-owned change queue.
    pub(super) fn buffer(
        &mut self,
        frames: usize,
        channels: usize,
        changes: &mut VecDeque<Signal>,
    ) -> Vec<f32> {
        let mut buffer = vec![0.0; frames * channels];
        for sample in buffer.chunks_exact_mut(channels) {
            let start = self.frame_time(self.frames);
            let end = self.frame_time(self.frames + 1);
            let mut integral = 0;
            while changes.front().is_some_and(|change| change.state.at <= end) {
                let change = changes.pop_front().unwrap();
                integral += self.signal.advance_to(change.state.at);
                self.signal = change;
            }
            integral += self.signal.advance_to(end);
            let duration = end.duration_since(start).as_nanos();
            sample.fill((0.2 * integral as f64 / (duration * u128::from(INPUT_HZ)) as f64) as f32);
            self.frames += 1;
        }
        buffer
    }
}
