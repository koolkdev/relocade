use crate::Pc;
use async_channel::Sender;
use relocade_mixer::Mixer;
use std::{cell::RefCell, rc::Rc};

// All sources start at unity gain, independently of the other installed sources.
// The mixer sums with headroom, and platform output clamps the final sample.
pub(super) const FM_GAIN: f32 = 1.0;
pub(super) const PCM_GAIN: f32 = 1.0;
const SPEAKER_GAIN: f32 = 1.0;

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
            card.add_audio_sources(&mut mixer, config.sample_rate)?;
        }
        let mut speaker = self.speaker.borrow().audio_source();
        mixer.add_source(SPEAKER_GAIN, move |interval| speaker.sample(interval));
        // Connection belongs to the session even if this await is cancelled.
        self.audio_attached = true;
        self.runner
            .connect(Rc::new(RefCell::new(mixer)), |_| {})
            .await;
        Ok(())
    }
}
