//! Sound Blaster 2.0 DSP and its shared AdLib-compatible FM chip.
#![forbid(unsafe_code)]

mod dsp;

use dsp::Dsp;
use relocade_adlib::Adlib;
use relocade_dma::Dma;
use relocade_machine::{Clock, Device, DeviceConnection, IoBus, Memory, Runner};
use relocade_pic::Pic;
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
    dsp: Rc<RefCell<Dsp>>,
    adlib: Rc<RefCell<Adlib>>,
}

impl SoundBlaster {
    pub fn new(clock: Clock, memory: Memory, io: &IoBus, dma: Rc<RefCell<Dma>>, pic: &Pic) -> Self {
        Self {
            adlib: Rc::new(RefCell::new(Adlib::new(clock.clone()))),
            dsp: Rc::new(RefCell::new(Dsp::new(clock, memory, io, dma, pic))),
        }
    }

    pub async fn initialize(
        runner: &Runner,
        dma: Rc<RefCell<Dma>>,
        pic: &Pic,
    ) -> Rc<RefCell<Self>> {
        let card = Rc::new(RefCell::new(Self::new(
            runner.clock().clone(),
            runner.memory(),
            &runner.io(),
            dma,
            pic,
        )));
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
        Dsp::configure_io(io, &card.borrow().dsp);
    }

    /// The same FM chip responds at both 228h/229h and 388h/389h.
    pub fn adlib(&self) -> Rc<RefCell<Adlib>> {
        self.adlib.clone()
    }
}

impl Device for SoundBlaster {
    async fn run(device: Rc<RefCell<Self>>) {
        let dsp = device.borrow().dsp.clone();
        Dsp::run(dsp).await;
    }
}
