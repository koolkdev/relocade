//! Generate speaker PCM on a shared audio frame grid.
use crate::Speaker;
use relocade_frontend::audio::AudioTiming;
use relocade_pit::{Channel2State, Counter, INPUT_HZ};
use std::{cell::RefCell, collections::VecDeque, ops::Range, rc::Rc};

struct Change {
    state: Channel2State,
    enabled: bool,
}

struct Drive {
    counter: Counter,
    enabled: bool,
    // One native tick is sample_rate units; one audio frame is native_rate units.
    position: i128,
    // Fraction of a PIT input cycle, with denominator native_rate * sample_rate.
    fraction: u128,
    timing: AudioTiming,
}

impl Drive {
    fn new(change: Change, timing: AudioTiming) -> Self {
        Self {
            position: Self::position(timing, change.state.at),
            fraction: u128::from(change.state.fraction) * u128::from(timing.sample_rate()),
            counter: change.state.counter,
            enabled: change.enabled,
            timing,
        }
    }

    fn position(timing: AudioTiming, at: relocade_machine::Timestamp) -> i128 {
        i128::from(at.ticks().wrapping_sub(timing.origin().ticks()) as i64)
            * i128::from(timing.sample_rate())
    }

    /// High time in PIT phase units, retaining partial input cycles locally.
    fn advance_to(&mut self, end: i128) -> u128 {
        if end <= self.position {
            return 0;
        }
        let denominator = u128::from(self.timing.tick_rate().ticks_per_second().get())
            * u128::from(self.timing.sample_rate());
        let span = (end - self.position) as u128 * u128::from(INPUT_HZ) + self.fraction;
        let before = u128::from(self.counter.output());
        let high = self
            .counter
            .advance(u64::try_from(span / denominator).expect("speaker progress fits"));
        let fraction = span % denominator;
        let integral = u128::from(high) * denominator
            + fraction * u128::from(self.counter.output())
            - self.fraction * before;
        self.position = end;
        self.fraction = fraction;
        if self.enabled {
            integral
        } else {
            0
        }
    }

    fn advance_recorded(&mut self, changes: &mut VecDeque<Change>, end: i128) -> u128 {
        let timing = self.timing;
        let mut integral = 0;
        while changes
            .front()
            .is_some_and(|change| Self::position(timing, change.state.at) <= end)
        {
            let change = changes.pop_front().unwrap();
            integral += self.advance_to(Self::position(timing, change.state.at));
            *self = Self::new(change, timing);
        }
        integral + self.advance_to(end)
    }
}

/// Mono speaker output in 0..=1, on the output's shared frame grid.
pub struct AudioSource {
    drive: Drive,
    changes: Rc<RefCell<VecDeque<Change>>>,
}

impl Speaker {
    /// Records PIT/control writes from the current state. Create sources before
    /// execution starts, with the same timing used by the mixer.
    pub fn audio_source(&self, timing: AudioTiming) -> AudioSource {
        let mut pit = self.pit.borrow_mut();
        let changes = Rc::new(RefCell::new(VecDeque::new()));
        let recorded = Rc::downgrade(&changes);
        let control = self.control.clone();
        pit.observe_channel2(move |state| {
            if let Some(changes) = recorded.upgrade() {
                changes.borrow_mut().push_back(Change {
                    state,
                    enabled: control.get() & 2 != 0,
                });
            }
        });
        let initial = changes.borrow_mut().pop_front().unwrap();
        AudioSource {
            drive: Drive::new(initial, timing),
            changes,
        }
    }
}

impl AudioSource {
    /// Fill one mono sample per frame. Ranges progress forward without overlap;
    /// skipped frames advance oscillator phase and consume recorded changes.
    pub fn generate_samples(&mut self, frames: Range<u64>, output: &mut [f32]) {
        assert_eq!(
            frames.end.checked_sub(frames.start),
            Some(output.len() as u64)
        );
        let rate = self.drive.timing.tick_rate().ticks_per_second().get();
        let mut changes = self.changes.borrow_mut();
        self.drive
            .advance_recorded(&mut changes, i128::from(frames.start) * i128::from(rate));
        for (frame, sample) in frames.zip(output) {
            let integral = self
                .drive
                .advance_recorded(&mut changes, i128::from(frame + 1) * i128::from(rate));
            *sample = (integral as f64 / (u128::from(rate) * u128::from(INPUT_HZ)) as f64) as f32;
        }
    }
}

#[cfg(test)]
mod tests;
