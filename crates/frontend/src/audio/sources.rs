use super::AudioTiming;
use std::ops::Range;

type GenerateSamples = dyn FnMut(Range<u64>, &mut [f32]);

struct Source {
    gain: f32,
    generate_samples: Box<GenerateSamples>,
}

/// Device-owned recorders and generators on one output frame grid.
/// Devices register their own composition; callers process every source alike.
pub struct Sources {
    timing: AudioTiming,
    sources: Vec<Source>,
    mono: Vec<f32>,
}

impl Sources {
    pub fn new(timing: AudioTiming) -> Self {
        Self {
            timing,
            sources: Vec::new(),
            mono: Vec::new(),
        }
    }

    pub fn timing(&self) -> AudioTiming {
        self.timing
    }

    /// Registers event capture and mono generation together. Capture runs on
    /// the device owner; owned events and synthesis state can move independently.
    /// Generation must fill every sample. Gains preserve headroom until playback.
    pub fn add_source<E: Send + 'static>(
        &mut self,
        gain: f32,
        mut record: impl FnMut() -> E + 'static,
        mut generate: impl FnMut(E, Range<u64>, &mut [f32]) + Send + 'static,
    ) {
        self.sources.push(Source {
            gain,
            generate_samples: Box::new(move |frames, output| {
                generate(record(), frames, output);
            }),
        });
    }

    /// Captures events and mixes a block into interleaved output on this owner.
    /// Each source receives the same absolute frame range once per block.
    pub fn generate_samples(&mut self, frames: Range<u64>, output: &mut [f32]) {
        let count = usize::try_from(
            frames
                .end
                .checked_sub(frames.start)
                .expect("forward frames"),
        )
        .expect("frame count fits");
        assert!(count != 0 && !output.is_empty() && output.len().is_multiple_of(count));
        let channels = output.len() / count;
        self.mono.resize(count, 0.0);
        output.fill(0.0);
        for source in &mut self.sources {
            (source.generate_samples)(frames.clone(), &mut self.mono);
            for (frame, &sample) in output.chunks_exact_mut(channels).zip(&self.mono) {
                for channel in frame {
                    *channel += source.gain * sample;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
