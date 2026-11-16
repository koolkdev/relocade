use super::playback::sample_delay;
use super::Dsp;
use relocade_frontend::audio::AudioTiming;
use relocade_machine::Timestamp;
use std::{
    cell::RefCell,
    collections::VecDeque,
    ops::Range,
    rc::{Rc, Weak},
    time::Duration,
};

struct Samples {
    origin: Timestamp,
    first_sample: u64,
    period: Duration,
    bytes: Vec<u8>,
    consumed: usize,
}

/// Held DAC output on the shared audio frame grid.
pub struct AudioSource {
    device: Weak<RefCell<Dsp>>,
    timing: AudioTiming,
    next_frame: u64,
    value: f64,
    samples: Rc<RefCell<VecDeque<Samples>>>,
}

pub(super) struct Dac {
    enabled: bool,
    output: u8,
    samples: Weak<RefCell<VecDeque<Samples>>>,
}

impl Default for Dac {
    fn default() -> Self {
        Self {
            enabled: false,
            output: 0x80,
            samples: Weak::new(),
        }
    }
}

impl Dac {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn reset(&mut self, at: Timestamp) {
        self.enabled = false;
        self.output = 0x80;
        self.record_output(at);
    }

    pub fn set_enabled(&mut self, at: Timestamp, enabled: bool) {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.record_output(at);
        }
    }

    pub fn write(&mut self, at: Timestamp, value: u8) {
        self.output = value;
        self.record_output(at);
    }

    pub fn write_samples(
        &mut self,
        origin: Timestamp,
        first_sample: u64,
        period: Duration,
        bytes: Vec<u8>,
    ) {
        if let Some(&last) = bytes.last() {
            self.output = last;
            if self.enabled {
                self.record(origin, first_sample, period, bytes);
            }
        }
    }

    pub fn source(&mut self, timing: AudioTiming, device: Weak<RefCell<Dsp>>) -> AudioSource {
        let samples = Rc::new(RefCell::new(VecDeque::new()));
        self.samples = Rc::downgrade(&samples);
        AudioSource {
            device,
            timing,
            next_frame: 0,
            value: pcm(self.level()),
            samples,
        }
    }

    fn level(&self) -> u8 {
        if self.enabled {
            self.output
        } else {
            0x80
        }
    }

    fn record(&self, origin: Timestamp, first_sample: u64, period: Duration, bytes: Vec<u8>) {
        if let Some(samples) = self.samples.upgrade() {
            samples.borrow_mut().push_back(Samples {
                origin,
                first_sample,
                period,
                bytes,
                consumed: 0,
            });
        }
    }

    fn record_output(&self, at: Timestamp) {
        self.record(at, 0, Duration::ZERO, vec![self.level()]);
    }
}

impl AudioSource {
    /// Fill one mono sample per frame. Consumed DMA bytes are owned here;
    /// generation never rereads memory. Future changes stay queued even when
    /// the rounded wakeup has already observed them on the execution worker.
    pub fn generate_samples(&mut self, frames: Range<u64>, output: &mut [f32]) {
        assert_eq!(
            frames.end.checked_sub(frames.start),
            Some(output.len() as u64)
        );
        assert!(
            frames.start >= self.next_frame,
            "DAC frames must progress forward"
        );
        if let Some(device) = self.device.upgrade() {
            // Device and mixer may wake in either order. Capture through now
            // before using the timeline; this synchronization owns DMA reads.
            device.borrow_mut().synchronize();
        }
        let mut samples = self.samples.borrow_mut();
        for (frame, sample) in frames.clone().zip(output) {
            let mut offset = 0.0;
            let mut integral = 0.0;
            while let Some(block) = samples.front_mut() {
                let delay = sample_delay(block.period, block.first_sample + block.consumed as u64);
                let Some(next) = self.timing.event_offset(frame, block.origin, delay) else {
                    break;
                };
                integral += (next - offset) * self.value;
                offset = next;
                self.value = pcm(block.bytes[block.consumed]);
                block.consumed += 1;
                if block.consumed == block.bytes.len() {
                    samples.pop_front();
                }
            }
            *sample = (integral + (1.0 - offset) * self.value) as f32;
        }
        self.next_frame = frames.end;
    }
}

fn pcm(value: u8) -> f64 {
    (f64::from(value) - 128.0) / 128.0
}

#[cfg(test)]
mod tests {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    use super::*;
    use relocade_machine::TickRate;
    use std::num::NonZeroU64;

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn scheduled_bytes_and_future_writes_survive_a_rounded_block_wakeup() {
        for batch in [1, 2, 5] {
            let timing = AudioTiming::new(
                Timestamp::from_ticks(0),
                TickRate::new(NonZeroU64::new(1000).unwrap()),
                4000,
            )
            .unwrap();
            let mut dac = Dac::default();
            let mut source = dac.source(timing, Weak::new());
            dac.set_enabled(timing.origin(), true);
            // DMA bytes at 125us and 250us cannot be represented as native ticks.
            dac.write_samples(timing.origin(), 1, Duration::from_micros(125), vec![0, 255]);
            // The wakeup at tick 1 also captures this later write; the first four
            // output frames must retain the preceding held level.
            dac.write(Timestamp::from_ticks(1), 128);
            let mut output = [0.0; 5];
            for (index, chunk) in output.chunks_mut(batch).enumerate() {
                let start = (index * batch) as u64;
                source.generate_samples(start..start + chunk.len() as u64, chunk);
            }
            assert_eq!(
                output,
                [-0.5, 127.0 / 128.0, 127.0 / 128.0, 127.0 / 128.0, 0.0]
            );
        }
    }
}
