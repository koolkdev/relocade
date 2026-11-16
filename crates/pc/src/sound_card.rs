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

    /// Registers the card's sources on the shared output frame grid.
    #[cfg(feature = "audio")]
    fn add_audio_sources(
        &self,
        sources: &mut relocade_frontend::audio::Sources,
    ) -> Result<(), String>;
}

impl SoundCard for Rc<RefCell<Adlib>> {
    fn settings(&self) -> Settings {
        Settings::AdLib {
            base_port: relocade_adlib::ADDRESS_STATUS_PORT,
        }
    }

    #[cfg(feature = "audio")]
    fn add_audio_sources(
        &self,
        sources: &mut relocade_frontend::audio::Sources,
    ) -> Result<(), String> {
        self.borrow_mut().add_audio_sources(sources)
    }
}

impl SoundCard for Rc<RefCell<SoundBlaster>> {
    fn settings(&self) -> Settings {
        Settings::SoundBlaster(self.borrow().settings())
    }

    #[cfg(feature = "audio")]
    fn add_audio_sources(
        &self,
        sources: &mut relocade_frontend::audio::Sources,
    ) -> Result<(), String> {
        self.borrow().add_audio_sources(sources)
    }
}
