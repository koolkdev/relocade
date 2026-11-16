use super::AudioTiming;
use std::{
    ops::Range,
    sync::{Arc, Mutex},
};

type GenerateSamples = Box<dyn FnOnce(Range<u64>, &mut [f32]) + Send>;

struct Source {
    gain: f32,
    record: Box<dyn FnMut() -> GenerateSamples>,
}

/// Device-owned recorders and retained generators on one output frame grid.
/// Devices register their own composition; callers process every source alike.
pub struct Sources {
    timing: AudioTiming,
    sources: Vec<Source>,
    mono: Arc<Mutex<Vec<f32>>>,
}

impl Sources {
    pub fn new(timing: AudioTiming) -> Self {
        Self {
            timing,
            sources: Vec::new(),
            mono: Arc::default(),
        }
    }

    pub fn timing(&self) -> AudioTiming {
        self.timing
    }

    /// Registers event capture and mono generation together. Capture runs on
    /// the device owner; calculations retain owned events and this generator.
    /// Generation must fill every sample. Gains preserve headroom until playback.
    pub fn add_source<E: Send + 'static>(
        &mut self,
        gain: f32,
        mut record: impl FnMut() -> E + 'static,
        generate: impl FnMut(E, Range<u64>, &mut [f32]) + Send + 'static,
    ) {
        let generate = Arc::new(Mutex::new(generate));
        self.sources.push(Source {
            gain,
            record: Box::new(move || {
                let events = record();
                let generate = generate.clone();
                Box::new(move |frames, output| {
                    // Only calculations access synthesis state, once per block.
                    generate.lock().unwrap()(events, frames, output);
                })
            }),
        });
    }

    /// Records events here and returns an owned calculation of interleaved PCM.
    /// Run calculations in recording order. Each source receives the same frame
    /// range once per block, retaining synthesis state and reusable scratch space.
    /// Dropping this collection stops recording; captured work can still finish.
    pub fn record(
        &mut self,
        frames: Range<u64>,
        channels: usize,
    ) -> impl FnOnce() -> Vec<f32> + Send + 'static {
        let count = usize::try_from(
            frames
                .end
                .checked_sub(frames.start)
                .expect("forward frames"),
        )
        .expect("frame count fits");
        assert!(count != 0 && channels != 0, "nonempty PCM block");
        let samples = count.checked_mul(channels).expect("PCM block fits");
        let recordings: Vec<_> = self
            .sources
            .iter_mut()
            .map(|source| (source.gain, (source.record)()))
            .collect();
        let mono = self.mono.clone();
        move || {
            let mut mono = mono.lock().unwrap();
            mono.resize(count, 0.0);
            let mut output = vec![0.0; samples];
            for (gain, generate) in recordings {
                generate(frames.clone(), &mut mono);
                for (frame, &sample) in output.chunks_exact_mut(channels).zip(mono.iter()) {
                    for channel in frame {
                        *channel += gain * sample;
                    }
                }
            }
            output
        }
    }
}

#[cfg(test)]
mod tests;
