use crate::{Adlib, Opl2};
use relocade_frontend::audio::AudioTiming;
use relocade_machine::Timestamp;
use std::{cell::RefCell, collections::VecDeque, ops::Range, rc::Rc, time::Duration};

pub(super) struct RegisterWrite {
    pub at: Timestamp,
    pub register: u8,
    pub value: u8,
}

/// Mono AdLib output on the shared audio frame grid.
/// Keep generating while output is muted or full; dropping the source stops recording.
pub struct AudioSource {
    timing: AudioTiming,
    chip: Opl2,
    next_frame: u64,
    value: f32,
    writes: Rc<RefCell<VecDeque<RegisterWrite>>>,
}

impl Adlib {
    /// Starts synthesis from current registers on the output's frame grid.
    /// Create before guest execution to retain the complete note timeline.
    /// A later attachment starts current voices with fresh envelopes.
    pub fn audio_source(&mut self, timing: AudioTiming) -> Result<AudioSource, String> {
        let mut chip = Opl2::new(timing.sample_rate())?;
        for (register, &value) in self.registers.iter().enumerate() {
            chip.write_register(register as u8, value);
        }
        let writes = Rc::new(RefCell::new(VecDeque::new()));
        self.writes = Rc::downgrade(&writes);
        Ok(AudioSource {
            timing,
            chip,
            next_frame: 0,
            value: 0.0,
            writes,
        })
    }
}

impl AudioSource {
    /// Fill one mono sample per frame. Skipped frames still advance envelopes.
    /// The OPL core already resamples at this rate: each value lasts one frame,
    /// and writes during that frame affect the next core step.
    pub fn generate_samples(&mut self, frames: Range<u64>, output: &mut [f32]) {
        assert_eq!(
            frames.end.checked_sub(frames.start),
            Some(output.len() as u64)
        );
        assert!(
            frames.start >= self.next_frame,
            "AdLib frames must progress forward"
        );
        let mut writes = self.writes.borrow_mut();
        for frame in self.next_frame..frames.end {
            if frame >= frames.start {
                output[(frame - frames.start) as usize] = self.value;
            }
            while writes.front().is_some_and(|write| {
                self.timing
                    .event_offset(frame, write.at, Duration::ZERO)
                    .is_some()
            }) {
                let write = writes.pop_front().unwrap();
                self.chip.write_register(write.register, write.value);
            }
            self.value = self.chip.sample();
        }
        self.next_frame = frames.end;
    }
}

#[cfg(test)]
mod tests;
