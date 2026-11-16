use crate::{Adlib, Opl2};
use relocade_frontend::audio::AudioTiming;
use relocade_machine::Timestamp;
use std::{cell::RefCell, collections::VecDeque, ops::Range, rc::Rc, time::Duration};

/// A register write at its native timestamp.
pub struct RegisterWrite {
    pub at: Timestamp,
    pub register: u8,
    pub value: u8,
}

/// Collects register writes on the device owner. Dropping it stops recording.
pub struct EventRecorder {
    writes: Rc<RefCell<VecDeque<RegisterWrite>>>,
}

/// Mono AdLib synthesis with owned events and no live device or clock.
pub struct SampleGenerator {
    timing: AudioTiming,
    chip: Opl2,
    next_frame: u64,
    value: f32,
    writes: VecDeque<RegisterWrite>,
}

impl Adlib {
    /// Registers this device's output at unity gain on the shared frame grid.
    pub fn add_audio_sources(
        &mut self,
        sources: &mut relocade_frontend::audio::Sources,
    ) -> Result<(), String> {
        let (mut recorder, mut generator) = self.audio_source(sources.timing())?;
        sources.add_source(
            1.0,
            move || recorder.take_events(),
            move |events, frames, output| generator.generate_samples(events, frames, output),
        );
        Ok(())
    }

    /// Starts synthesis from current registers on the output's frame grid.
    /// Create before guest execution to retain the complete note timeline.
    /// A later attachment starts current voices with fresh envelopes.
    pub fn audio_source(
        &mut self,
        timing: AudioTiming,
    ) -> Result<(EventRecorder, SampleGenerator), String> {
        let mut chip = Opl2::new(timing.sample_rate())?;
        for (register, &value) in self.registers.iter().enumerate() {
            chip.write_register(register as u8, value);
        }
        let writes = Rc::new(RefCell::new(VecDeque::new()));
        self.writes = Rc::downgrade(&writes);
        Ok((
            EventRecorder { writes },
            SampleGenerator {
                timing,
                chip,
                next_frame: 0,
                value: 0.0,
                writes: VecDeque::new(),
            },
        ))
    }
}

impl EventRecorder {
    /// Transfers recorded writes in order, including equal-timestamp writes.
    pub fn take_events(&mut self) -> VecDeque<RegisterWrite> {
        std::mem::take(&mut *self.writes.borrow_mut())
    }
}

impl SampleGenerator {
    /// Applies recorded writes and fills one mono sample per frame. Events must
    /// cover the requested range; future writes stay queued. Skipped frames still
    /// advance envelopes.
    /// The OPL core already resamples at this rate: each value lasts one frame,
    /// and writes during that frame affect the next core step.
    pub fn generate_samples(
        &mut self,
        events: VecDeque<RegisterWrite>,
        frames: Range<u64>,
        output: &mut [f32],
    ) {
        assert_eq!(
            frames.end.checked_sub(frames.start),
            Some(output.len() as u64)
        );
        assert!(
            frames.start >= self.next_frame,
            "AdLib frames must progress forward"
        );
        self.writes.extend(events);
        let writes = &mut self.writes;
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
