use relocade_frontend::audio::{AudioTiming, Config, Sources};
use relocade_machine::{
    worker::{JobExecutor, PoolError},
    Timestamp,
};

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

    pub async fn generate_samples(
        &mut self,
        sources: &mut Sources,
        jobs: &JobExecutor,
    ) -> Result<Vec<f32>, PoolError> {
        let end = self.next_frame + self.frames as u64;
        let samples = jobs
            .execute(sources.record(self.next_frame..end, self.channels))
            .await?;
        self.next_frame = end;
        Ok(samples)
    }
}
