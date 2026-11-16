use relocade_frontend::audio::{AudioTiming, Config, Sources};
use relocade_machine::Timestamp;

/// Retains output frame progress separately from scheduling and playback.
pub(super) struct BlockGenerator {
    timing: AudioTiming,
    next_frame: u64,
    frames: usize,
    channels: usize,
}

impl BlockGenerator {
    pub fn new(timing: AudioTiming, config: Config) -> Self {
        Self {
            timing,
            next_frame: 0,
            frames: config.block_frames.clamp(1, 256),
            channels: usize::from(config.channels),
        }
    }

    pub fn deadline(&self) -> Timestamp {
        self.timing
            .deadline_for_frame(self.next_frame + self.frames as u64)
    }

    pub fn generate_samples(&mut self, sources: &mut Sources) -> Vec<f32> {
        let end = self.next_frame + self.frames as u64;
        let mut output = vec![0.0; self.frames * self.channels];
        sources.generate_samples(self.next_frame..end, &mut output);
        self.next_frame = end;
        output
    }
}
