use crate::Speaker;
use async_channel::Sender;
use relocade_machine::Clock;
use relocade_pit::{Counter, INPUT_HZ};
use std::{cell::RefCell, rc::Rc};

impl<C: Clock> Speaker<C> {
    /// Attaches output once, including after the device activity has started.
    /// Synthesis runs on the execution worker. Connect a bounded channel
    /// consumed at the configured playback rate: backpressure provides scheduling
    /// cooperation. Unpaced sinks require external pacing. Muted output submits
    /// silence.
    pub fn attach_audio(
        &mut self,
        config: cpal::StreamConfig,
        samples: Sender<Vec<f32>>,
    ) -> Result<(), String> {
        if self.audio.is_some() {
            return Err("speaker output is already attached".into());
        }
        if config.sample_rate == 0 || config.channels == 0 {
            return Err("speaker output needs a nonzero sample rate and channel count".into());
        }
        if samples.capacity().is_none() {
            return Err("speaker output needs a bounded sample channel".into());
        }
        self.sync_to_clock();
        self.audio = Some(Audio {
            config,
            samples,
            counter: self.counter.clone(),
            fraction: 0,
        });
        self.audio_ready.0.try_send(()).unwrap();
        Ok(())
    }
}

pub(super) struct Audio {
    config: cpal::StreamConfig,
    samples: Sender<Vec<f32>>,
    // Playback advances ahead of the machine clock by the queued audio duration.
    counter: Counter,
    // Fraction of one PIT clock, measured in units of the output sample rate.
    fraction: u64,
}

impl Audio {
    pub(super) fn update(&mut self, counter: &Counter) {
        self.counter = counter.clone();
        self.fraction = 0;
    }

    fn buffer(&mut self, enabled: bool) -> Vec<f32> {
        let batch = match self.config.buffer_size {
            cpal::BufferSize::Fixed(n) => n.clamp(1, 256) as usize,
            cpal::BufferSize::Default => 256,
        };
        let channels = usize::from(self.config.channels);
        let mut buffer = vec![0.0; batch * channels];
        if enabled {
            let rate = u64::from(self.config.sample_rate);
            for output in buffer.chunks_exact_mut(channels) {
                let initial_high = u64::from(self.counter.output());
                let span = self.fraction + INPUT_HZ;
                let high = self.counter.advance(span / rate);
                let next_fraction = span % rate;
                // Integrate the whole clocks plus the two fractional endpoints.
                let integral = high * rate + next_fraction * u64::from(self.counter.output())
                    - self.fraction * initial_high;
                output.fill((0.2 * integral as f64 / INPUT_HZ as f64) as f32);
                self.fraction = next_fraction;
            }
        }
        buffer
    }
}

pub(super) async fn run<C: Clock>(device: Rc<RefCell<Speaker<C>>>) {
    let ready = device.borrow().audio_ready.1.clone();
    ready.recv().await.unwrap();
    let samples = device.borrow().audio.as_ref().unwrap().samples.clone();
    loop {
        let buffer = {
            let mut device = device.borrow_mut();
            let enabled = device.control & 2 != 0;
            device.audio.as_mut().unwrap().buffer(enabled)
        };
        if samples.send(buffer).await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
