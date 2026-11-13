use crate::{Adlib, Opl2};
use relocade_machine::{Clock, Timestamp};
use std::{cell::RefCell, collections::VecDeque, ops::Range, rc::Rc};

pub(super) struct RegisterWrite {
    pub at: Timestamp,
    pub register: u8,
    pub value: u8,
}

/// Clocked mono AdLib samples for the mixer's elapsed-interval callback.
/// Keep polling while output is muted or full; dropping the source stops recording.
pub struct AudioSource {
    clock: Clock,
    chip: Opl2,
    at: Timestamp,
    rate: u32,
    fraction: u32,
    value: f32,
    writes: Rc<RefCell<VecDeque<RegisterWrite>>>,
}

impl Adlib {
    /// Starts synthesis from the current registers at the mixer's output rate.
    /// Create the source before guest execution to preserve the complete note
    /// timeline. A later attachment starts current voices with fresh envelopes.
    /// Replaces any previous source; dropping it releases pending writes.
    pub fn audio_source(&mut self, sample_rate: u32) -> Result<AudioSource, String> {
        let mut chip = Opl2::new(sample_rate)?;
        for (register, &value) in self.registers.iter().enumerate() {
            chip.write_register(register as u8, value);
        }
        let writes = Rc::new(RefCell::new(VecDeque::new()));
        self.writes = Rc::downgrade(&writes);
        Ok(AudioSource {
            clock: self.clock.clone(),
            chip,
            at: self.clock.now(),
            rate: sample_rate,
            fraction: 0,
            value: 0.0,
            writes,
        })
    }
}

impl AudioSource {
    /// Averages resampled PCM over an elapsed interval on the device's clock.
    /// Intervals progress forward from source creation without overlap and span
    /// at least one nanosecond. Skipped time still advances synthesis and writes.
    pub fn sample(&mut self, interval: Range<Timestamp>) -> f32 {
        self.advance_to(interval.start);
        let start = self.at;
        let integral = self.advance_to(interval.end);
        let duration = self.clock.duration_since(self.at, start).as_nanos();
        (integral / (duration * u128::from(self.rate)) as f64) as f32
    }

    fn advance_to(&mut self, end: Timestamp) -> f64 {
        let mut integral = 0.0;
        while self
            .writes
            .borrow()
            .front()
            .is_some_and(|write| write.at <= end)
        {
            let write = self.writes.borrow_mut().pop_front().unwrap();
            integral += self.advance_clock(write.at);
            self.chip.write_register(write.register, write.value);
        }
        integral + self.advance_clock(end)
    }

    fn advance_clock(&mut self, end: Timestamp) -> f64 {
        let elapsed = self.clock.consume_elapsed(&mut self.at, end);
        let mut remaining = elapsed.as_nanos() * u128::from(self.rate);
        let mut integral = 0.0;
        // The core resamples to the configured rate. Hold each resulting PCM
        // sample until the next boundary, retaining phase across writes/polls.
        while remaining != 0 {
            let span = remaining.min(u128::from(1_000_000_000 - self.fraction)) as u32;
            integral += f64::from(self.value) * f64::from(span);
            remaining -= u128::from(span);
            self.fraction += span;
            if self.fraction == 1_000_000_000 {
                self.fraction = 0;
                self.value = self.chip.sample();
            }
        }
        integral
    }
}

#[cfg(test)]
mod tests;
