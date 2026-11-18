use crate::Pc;
use async_channel::Sender;
use futures_lite::future::or;
use relocade_frontend::audio::{AudioTiming, Config, Sources};
use relocade_machine::{worker::JobExecutor, Clock};

mod generation;
use generation::BlockGenerator;

/// Records elapsed device events and submits complete PCM blocks to playback.
struct Audio {
    clock: Clock,
    sources: Sources,
    generator: BlockGenerator,
    samples: Sender<Vec<f32>>,
}

impl Pc {
    /// Attaches one bounded PCM stream with synthesis on a reusable job thread.
    /// Attach before guest execution to capture the complete note timeline.
    pub async fn attach_audio(
        &mut self,
        config: Config,
        samples: Sender<Vec<f32>>,
    ) -> Result<(), String> {
        if self.audio_attached {
            return Err("PC audio output is already attached".into());
        }
        let mut audio = Audio::new(self.clock().clone().into(), config, samples)?;
        if let Some(card) = &self.sound_card {
            card.add_audio_sources(&mut audio.sources)?;
        }
        self.speaker.borrow().add_audio_sources(&mut audio.sources);
        let jobs = self.runner.spawn_job_thread().await;
        // The activity and job thread belong to the session before this await.
        self.audio_attached = true;
        self.runner.spawn_activity(audio.run(jobs)).await;
        Ok(())
    }
}

impl Audio {
    fn new(clock: Clock, config: Config, samples: Sender<Vec<f32>>) -> Result<Self, String> {
        if config.channels == 0 {
            return Err("PC audio needs at least one channel".into());
        }
        if samples.capacity().is_none() {
            return Err("PC audio needs a bounded sample channel".into());
        }
        let timing = AudioTiming::new(clock.now(), clock.tick_rate(), config.sample_rate)?;
        Ok(Self {
            clock,
            sources: Sources::new(timing),
            generator: BlockGenerator::new(timing, config),
            samples,
        })
    }

    async fn run(mut self, jobs: JobExecutor) {
        while !self.samples.is_closed() {
            or(
                self.clock.sleep_until(self.generator.deadline()),
                self.samples.closed(),
            )
            .await;
            if self.samples.is_closed() {
                return;
            }
            let calculation = self.generator.generate_samples(&mut self.sources, &jobs);
            let result = or(async { calculation.await.ok() }, async {
                self.samples.closed().await;
                None
            })
            .await;
            let Some(buffer) = result else {
                return;
            };
            // A stalled playback consumer keeps recent PCM without blocking hardware.
            if self.samples.force_send(buffer).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;
