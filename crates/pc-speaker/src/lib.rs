//! PC speaker control and synthesis from PIT channel 2.

#[cfg(feature = "audio")]
mod audio;
#[cfg(feature = "audio")]
pub use audio::AudioSource;

use relocade_machine::{Device, DeviceCallbacks, DeviceConnection, PortRead, PortWrite, Runner};
use relocade_pit::Pit;
use std::{
    cell::{Cell, RefCell},
    ops::RangeInclusive,
    rc::Rc,
};

/// PC/AT system control port B, including the speaker gate and data bits.
pub const SPEAKER_CONTROL_PORT: u16 = 0x61;
pub const PORT_RANGE: RangeInclusive<u16> = SPEAKER_CONTROL_PORT..=SPEAKER_CONTROL_PORT;

/// Execution-owned speaker gate and drive, connected to PIT channel 2.
pub struct Speaker {
    pit: Rc<RefCell<Pit>>,
    control: Rc<Cell<u8>>,
}

impl Speaker {
    /// Creates the speaker and registers its control port.
    pub async fn initialize(runner: &Runner, pit: Rc<RefCell<Pit>>) -> Rc<RefCell<Self>> {
        let speaker = Rc::new(RefCell::new(Self::new(pit)));
        runner
            .connect(speaker.clone(), |io| Self::configure_io(io, &speaker))
            .await;
        speaker
    }

    /// Bind the byte-only system control register.
    pub fn configure_io(io: &mut DeviceConnection<'_>, speaker: &Rc<RefCell<Self>>) {
        let callbacks = DeviceCallbacks::new(speaker);
        io.bind(
            SPEAKER_CONTROL_PORT,
            &PortRead::byte(callbacks.read(|_, speaker| speaker.read_control())),
            &PortWrite::byte(callbacks.write(|_, speaker, value| speaker.write_control(value))),
        );
    }

    pub fn new(pit: Rc<RefCell<Pit>>) -> Self {
        Self {
            pit,
            control: Rc::new(Cell::new(0)),
        }
    }

    /// Digital speaker drive: [`SPEAKER_CONTROL_PORT`] bit 1 AND timer OUT2.
    pub fn output(&mut self) -> bool {
        let output = self.pit.borrow_mut().channel2_output();
        self.control.get() & 2 != 0 && output
    }
    fn read_control(&mut self) -> u8 {
        let output = self.pit.borrow_mut().channel2_output();
        self.control.get() | (u8::from(output) << 5)
    }

    fn write_control(&mut self, value: u8) {
        let control = value & 3;
        if self.control.get() == control {
            return;
        }
        self.control.set(control);
        self.pit.borrow_mut().set_channel2_gate(control & 1 != 0);
    }
}

impl Device for Speaker {}
