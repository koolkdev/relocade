//! Clocked audio mixing on an execution runner, independent of guest hardware.
#![forbid(unsafe_code)]

use async_channel::Sender;
use futures_lite::future::or;
use relocade_machine::{Clock, Device, Timestamp};
use std::{cell::RefCell, ops::Range, rc::Rc, time::Duration};

struct Source {
    gain: f32,
    sample: Box<dyn FnMut(Range<Timestamp>) -> f32>,
}

/// Mixes elapsed source samples into one bounded, interleaved PCM stream.
/// Configure sources before connecting this activity with [`relocade_machine::Runner::connect`].
/// The runner owns its lifetime; platform playback stays with the embedding.
pub struct Mixer {
    clock: Clock,
    config: cpal::StreamConfig,
    samples: Sender<Vec<f32>>,
    sources: Vec<Source>,
}

impl Mixer {
    /// Creates output on the supplied emulation clock. Samples are interleaved
    /// f32 values at the CPAL configuration's rate and channel count.
    /// Requires a bounded channel, a rate in 1..=1,000,000,000, and nonzero channels.
    pub fn new(
        clock: Clock,
        config: cpal::StreamConfig,
        samples: Sender<Vec<f32>>,
    ) -> Result<Self, String> {
        if config.sample_rate == 0 || config.sample_rate > 1_000_000_000 || config.channels == 0 {
            return Err(
                "audio mixer needs a representable sample rate and nonzero channel count".into(),
            );
        }
        if samples.capacity().is_none() {
            return Err("audio mixer needs a bounded sample channel".into());
        }
        Ok(Self {
            clock,
            config,
            samples,
            sources: Vec::new(),
        })
    }

    /// Adds a mono source before the activity starts. The callback averages its
    /// signal over each elapsed interval on this mixer's clock, without blocking.
    /// Gains are applied before summing; the result feeds every output channel.
    pub fn add_source(&mut self, gain: f32, sample: impl FnMut(Range<Timestamp>) -> f32 + 'static) {
        self.sources.push(Source {
            gain,
            sample: Box::new(sample),
        });
    }
}

impl Device for Mixer {
    async fn run(device: Rc<RefCell<Self>>) {
        // The activity owns sources so closing output releases recorded writes.
        let mut sources = std::mem::take(&mut device.borrow_mut().sources);
        let clock = device.borrow().clock.clone();
        let config = device.borrow().config;
        let samples = device.borrow().samples.clone();
        let started_at = clock.now();
        let rate = u64::from(config.sample_rate);
        let frame_time = |frame: u64| {
            clock.add(
                started_at,
                Duration::new(
                    frame / rate,
                    ((frame % rate) * 1_000_000_000).div_ceil(rate) as u32,
                ),
            )
        };
        let batch = match config.buffer_size {
            cpal::BufferSize::Fixed(frames) => frames.clamp(1, 256) as usize,
            cpal::BufferSize::Default => 256,
        };
        let channels = usize::from(config.channels);
        let mut frames = 0;
        while !samples.is_closed() {
            or(
                clock.sleep_until(frame_time(frames + batch as u64)),
                samples.closed(),
            )
            .await;
            if samples.is_closed() {
                return;
            }
            let mut buffer = vec![0.0; batch * channels];
            for frame in buffer.chunks_exact_mut(channels) {
                let interval = frame_time(frames)..frame_time(frames + 1);
                let sample = sources
                    .iter_mut()
                    .map(|source| source.gain * (source.sample)(interval.clone()))
                    .sum();
                // Preserve headroom until platform output converts the mixed sample.
                frame.fill(sample);
                frames += 1;
            }
            // A stalled sink retains recent PCM without blocking device progress.
            if samples.force_send(buffer).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;
