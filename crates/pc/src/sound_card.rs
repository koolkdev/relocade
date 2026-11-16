#[cfg(feature = "audio")]
use crate::audio::{FM_GAIN, PCM_GAIN};
use relocade_adlib::Adlib;
use relocade_sound_blaster::SoundBlaster;
pub use relocade_sound_blaster::{Model as SoundBlasterModel, Settings as SoundBlasterSettings};
use std::{cell::RefCell, rc::Rc};

/// Hardware settings reported by an installed sound card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settings {
    AdLib { base_port: u16 },
    SoundBlaster(SoundBlasterSettings),
}

/// Hardware settings and audio sources provided by an installed PC sound card.
pub trait SoundCard {
    /// Describes the installed hardware independently of guest operating systems.
    fn settings(&self) -> Settings;

    /// Registers the card's sources at the output rate using PC audio levels.
    #[cfg(feature = "audio")]
    fn add_audio_sources(&self, mixer: &mut relocade_mixer::Mixer) -> Result<(), String>;
}

impl SoundCard for Rc<RefCell<Adlib>> {
    fn settings(&self) -> Settings {
        Settings::AdLib {
            base_port: relocade_adlib::ADDRESS_STATUS_PORT,
        }
    }

    #[cfg(feature = "audio")]
    fn add_audio_sources(&self, mixer: &mut relocade_mixer::Mixer) -> Result<(), String> {
        let (mut recorder, mut source) = self.borrow_mut().audio_source(mixer.timing())?;
        mixer.add_source(FM_GAIN, move |frames, output| {
            source.generate_samples(recorder.take_events(), frames, output)
        });
        Ok(())
    }
}

impl SoundCard for Rc<RefCell<SoundBlaster>> {
    fn settings(&self) -> Settings {
        Settings::SoundBlaster(self.borrow().settings())
    }

    #[cfg(feature = "audio")]
    fn add_audio_sources(&self, mixer: &mut relocade_mixer::Mixer) -> Result<(), String> {
        self.borrow().adlib().add_audio_sources(mixer)?;
        let (mut recorder, mut pcm) = self.borrow().audio_source(mixer.timing());
        mixer.add_source(PCM_GAIN, move |frames, output| {
            pcm.generate_samples(recorder.take_events(), frames, output)
        });
        Ok(())
    }
}
