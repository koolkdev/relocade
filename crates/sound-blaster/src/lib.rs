//! Sound Blaster 2.0 DSP and its shared AdLib-compatible FM chip.
#![forbid(unsafe_code)]

mod dsp;

use dsp::Dsp;
use relocade_adlib::Adlib;
use relocade_machine::{
    Clock, Device, DeviceCallbacks, DeviceConnection, PortRead, PortWrite, Runner,
};
use std::{cell::RefCell, rc::Rc};

pub const BASE_PORT: u16 = 0x220;
pub const RESET_PORT: u16 = BASE_PORT + 6;
pub const FM_ADDRESS_STATUS_PORT: u16 = BASE_PORT + 8;
pub const FM_DATA_PORT: u16 = BASE_PORT + 9;
pub const READ_DATA_PORT: u16 = BASE_PORT + 0x0a;
pub const WRITE_DATA_PORT: u16 = BASE_PORT + 0x0c;
pub const WRITE_STATUS_PORT: u16 = WRITE_DATA_PORT;
pub const READ_STATUS_PORT: u16 = BASE_PORT + 0x0e;
pub const IRQ: u8 = 7;
pub const DMA_CHANNEL: u8 = 1;

/// Execution-owned SB2 card with one DSP and one OPL2 chip.
pub struct SoundBlaster {
    clock: Clock,
    dsp: Dsp,
    adlib: Rc<RefCell<Adlib>>,
}

impl SoundBlaster {
    pub fn new(clock: Clock) -> Self {
        Self {
            adlib: Rc::new(RefCell::new(Adlib::new(clock.clone()))),
            clock,
            dsp: Dsp::default(),
        }
    }

    pub async fn initialize(runner: &Runner) -> Rc<RefCell<Self>> {
        let card = Rc::new(RefCell::new(Self::new(runner.clock().clone())));
        runner
            .connect(card.clone(), |io| Self::configure_io(io, &card))
            .await;
        card
    }

    pub fn configure_io(io: &mut DeviceConnection<'_>, card: &Rc<RefCell<Self>>) {
        let adlib = card.borrow().adlib();
        for port in [FM_ADDRESS_STATUS_PORT, relocade_adlib::ADDRESS_STATUS_PORT] {
            Adlib::configure_io(io, &adlib, port);
        }
        let callbacks = DeviceCallbacks::new(card);
        io.bind(
            RESET_PORT,
            &PortRead::byte(|_| 0xff),
            &PortWrite::byte(callbacks.write(|_, card, value| {
                card.dsp.reset(&card.clock, value);
            })),
        );
        io.bind(
            READ_DATA_PORT,
            &PortRead::byte(callbacks.read(|_, card| card.dsp.read_data(&card.clock))),
            &PortWrite::byte(|_, _| {}),
        );
        io.bind(
            WRITE_DATA_PORT,
            &PortRead::byte(callbacks.read(|_, card| card.dsp.write_status(&card.clock))),
            &PortWrite::byte(callbacks.write(|_, card, value| {
                card.dsp.write(&card.clock, value);
            })),
        );
        io.bind(
            READ_STATUS_PORT,
            &PortRead::byte(callbacks.read(|_, card| card.dsp.read_status(&card.clock))),
            &PortWrite::byte(|_, _| {}),
        );
    }

    /// The same FM chip responds at both 228h/229h and 388h/389h.
    pub fn adlib(&self) -> Rc<RefCell<Adlib>> {
        self.adlib.clone()
    }
}

impl Device for SoundBlaster {}
