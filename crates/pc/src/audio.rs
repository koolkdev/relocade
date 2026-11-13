use crate::Pc;
use async_channel::Sender;
use relocade_mixer::Mixer;
use std::{cell::RefCell, rc::Rc};

impl Pc {
    /// Attaches one mixed output on the execution worker. The PC clock drives
    /// synthesis; the bounded channel supplies interleaved PCM to playback.
    /// Attach before executing guest code to capture the complete AdLib timeline.
    pub async fn attach_audio(
        &mut self,
        config: cpal::StreamConfig,
        samples: Sender<Vec<f32>>,
    ) -> Result<(), String> {
        if self.audio_attached {
            return Err("PC audio output is already attached".into());
        }
        let mut mixer = Mixer::new(self.clock().clone(), config, samples)?;
        if let Some(card) = &self.sound_card {
            card.add_audio_sources(&mut mixer, config.sample_rate, 0.8)?;
        }
        let mut speaker = self.speaker.borrow().audio_source();
        mixer.add_source(0.2, move |interval| speaker.sample(interval));
        // Connection belongs to the session even if this await is cancelled.
        self.audio_attached = true;
        self.runner
            .connect(Rc::new(RefCell::new(mixer)), |_| {})
            .await;
        Ok(())
    }
}
