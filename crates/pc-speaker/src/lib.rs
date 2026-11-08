//! PC speaker control and synthesis from PIT channel 2.

#[cfg(feature = "audio")]
mod audio;

use relocade_machine::{Device, DeviceConnection, Runner};
use relocade_pit::Pit;
use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};

/// PC/AT system control port B, including the speaker gate and data bits.
pub const SPEAKER_CONTROL_PORT: u16 = 0x61;
pub const PORT_RANGE: RangeInclusive<u16> = SPEAKER_CONTROL_PORT..=SPEAKER_CONTROL_PORT;

/// Execution-owned speaker gate and drive, connected to PIT channel 2.
pub struct Speaker {
    pit: Rc<RefCell<Pit>>,
    control: u8,
    #[cfg(feature = "audio")]
    audio: Option<audio::Audio>,
    #[cfg(feature = "audio")]
    audio_ready: (async_channel::Sender<()>, async_channel::Receiver<()>),
}

impl Speaker {
    /// Creates the speaker and registers its control port and playback activity.
    /// Returns the speaker handle and its port connection for the CPU bus.
    pub async fn initialize(
        runner: &Runner,
        pit: Rc<RefCell<Pit>>,
    ) -> (Rc<RefCell<Self>>, DeviceConnection) {
        let speaker = Rc::new(RefCell::new(Self::new(pit)));
        let connection = runner.connect(speaker.clone(), [], [PORT_RANGE]).await;
        (speaker, connection)
    }

    pub fn new(pit: Rc<RefCell<Pit>>) -> Self {
        Self {
            pit,
            control: 0,
            #[cfg(feature = "audio")]
            audio: None,
            #[cfg(feature = "audio")]
            audio_ready: async_channel::bounded(1),
        }
    }

    /// Digital speaker drive: [`SPEAKER_CONTROL_PORT`] bit 1 AND timer OUT2.
    pub fn output(&mut self) -> bool {
        let output = self.pit.borrow_mut().channel2_output();
        self.control & 2 != 0 && output
    }
}

impl Device for Speaker {
    #[cfg(feature = "audio")]
    async fn run(device: Rc<RefCell<Self>>) {
        audio::run(device).await;
    }

    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        match (port, bytes) {
            (SPEAKER_CONTROL_PORT, 1) => {
                let output = self.pit.borrow_mut().channel2_output();
                (self.control | (u8::from(output) << 5)).into()
            }
            _ => u32::MAX,
        }
    }

    fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        if port != SPEAKER_CONTROL_PORT || bytes != 1 {
            return;
        }
        let control = value as u8 & 3;
        if self.control == control {
            return;
        }
        self.control = control;
        self.pit.borrow_mut().set_channel2_gate(control & 1 != 0);
    }
}
