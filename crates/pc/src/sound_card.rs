use relocade_adlib::Adlib;
use relocade_sound_blaster::SoundBlaster;
use std::{cell::RefCell, rc::Rc};

/// Audio sources provided by a PC sound card. Initialization connects its hardware.
pub trait SoundCard {
    /// Registers the card's sources at the output rate, sharing the supplied gain.
    #[cfg(feature = "audio")]
    fn add_audio_sources(
        &self,
        mixer: &mut relocade_mixer::Mixer,
        sample_rate: u32,
        gain: f32,
    ) -> Result<(), String>;
}

impl SoundCard for Rc<RefCell<Adlib>> {
    #[cfg(feature = "audio")]
    fn add_audio_sources(
        &self,
        mixer: &mut relocade_mixer::Mixer,
        sample_rate: u32,
        gain: f32,
    ) -> Result<(), String> {
        let mut source = self.borrow_mut().audio_source(sample_rate)?;
        mixer.add_source(gain, move |interval| source.sample(interval));
        Ok(())
    }
}

impl SoundCard for Rc<RefCell<SoundBlaster>> {
    #[cfg(feature = "audio")]
    fn add_audio_sources(
        &self,
        mixer: &mut relocade_mixer::Mixer,
        sample_rate: u32,
        gain: f32,
    ) -> Result<(), String> {
        self.borrow()
            .adlib()
            .add_audio_sources(mixer, sample_rate, gain)
    }
}
