use super::Dsp;
use relocade_machine::{Clock, Timestamp};
use std::{
    cell::RefCell,
    collections::VecDeque,
    ops::Range,
    rc::{Rc, Weak},
    time::Duration,
};

struct Samples {
    at: Timestamp,
    period: Duration,
    bytes: Vec<u8>,
    consumed: usize,
}

/// Elapsed mono DAC output for the shared mixer's sample-interval callback.
pub struct AudioSource {
    device: Weak<RefCell<Dsp>>,
    clock: Clock,
    at: Timestamp,
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

    pub fn write_samples(&mut self, at: Timestamp, period: Duration, bytes: Vec<u8>) {
        if let Some(&last) = bytes.last() {
            self.output = last;
            if self.enabled {
                self.record(at, period, bytes);
            }
        }
    }

    pub fn source(&mut self, clock: Clock, device: Weak<RefCell<Dsp>>) -> AudioSource {
        let samples = Rc::new(RefCell::new(VecDeque::new()));
        self.samples = Rc::downgrade(&samples);
        AudioSource {
            device,
            at: clock.now(),
            clock,
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

    fn record(&self, at: Timestamp, period: Duration, bytes: Vec<u8>) {
        if let Some(samples) = self.samples.upgrade() {
            samples.borrow_mut().push_back(Samples {
                at,
                period,
                bytes,
                consumed: 0,
            });
        }
    }

    fn record_output(&self, at: Timestamp) {
        self.record(at, Duration::ZERO, vec![self.level()]);
    }
}

impl AudioSource {
    /// Averages held DAC levels over an elapsed interval on the device clock.
    /// Intervals progress forward without overlap and span at least one ns.
    /// Consumed DMA bytes are owned here; rendering never rereads those bytes.
    pub fn sample(&mut self, interval: Range<Timestamp>) -> f32 {
        if let Some(device) = self.device.upgrade() {
            // Mixer and device activities can wake in either order. Synchronize
            // hardware once through now before observing its recorded timeline.
            device.borrow_mut().synchronize();
        }
        self.advance_to(interval.start);
        let start = self.at;
        let integral = self.advance_to(interval.end);
        let duration = self.clock.duration_since(self.at, start).as_nanos();
        (integral / duration as f64) as f32
    }

    fn advance_to(&mut self, end: Timestamp) -> f64 {
        let mut integral = 0.0;
        let mut samples = self.samples.borrow_mut();
        while let Some(block) = samples.front_mut() {
            if block.at > end {
                break;
            }
            integral += self
                .clock
                .consume_elapsed(&mut self.at, block.at)
                .as_nanos() as f64
                * self.value;
            self.value = pcm(block.bytes[block.consumed]);
            block.consumed += 1;
            if block.consumed == block.bytes.len() {
                samples.pop_front();
            } else {
                block.at = self.clock.add(block.at, block.period);
            }
        }
        integral + self.clock.consume_elapsed(&mut self.at, end).as_nanos() as f64 * self.value
    }
}

fn pcm(value: u8) -> f64 {
    (f64::from(value) - 128.0) / 128.0
}
