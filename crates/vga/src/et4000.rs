#![doc = include_str!("et4000/README.md")]
mod configuration;
mod device;
mod effects;
mod mmio;
mod output;
mod ports;
mod ramdac;
mod registers;
mod scanout;
mod state;
mod timing;
mod viewport;

pub use state::Et4000State;

use crate::{
    memory::VideoMemory, output::Output, scanout::DisplayConfiguration, DisplaySnapshot, Registers,
    VgaCore, VgaCoreState,
};
use relocade_machine::{Clock, Runner};
use std::{cell::RefCell, rc::Rc};

/// Physical DRAM installed on this ET4000AX profile.
pub const VIDEO_MEMORY_BYTES: u32 = 1024 * 1024;

// EV8913's F151A input wiring, ordered by CS2:CS1:CS0.
const PIXEL_CLOCKS: [u64; 8] = [
    25_175_000, 28_322_000, 32_514_000, 40_000_000, 25_175_000, 28_322_000, 32_514_000, 44_900_000,
];

/// The ET4000AX card owns one VGA core, physical memory and display lifetime.
/// Its standard VGA interface is available independently of DISPI firmware.
pub struct Et4000Ax {
    vga: VgaCore,
    extensions: registers::Extensions,
    ramdac: ramdac::Sc11483,
    display_start_high: u8,
    memory: VideoMemory,
    output: Output,
    io: Option<crate::io::Io>,
}

impl Et4000Ax {
    pub async fn initialize(runner: &Runner) -> Rc<RefCell<Self>> {
        let card = Rc::new(RefCell::new(Self::with_clock(runner.clock().clone())));
        runner
            .connect(card.clone(), |io| Self::configure_io(io, &card))
            .await;
        card
    }

    /// Start from the common legacy preset. Firmware supplies fonts and a mode.
    pub fn with_clock(clock: Clock) -> Self {
        let vga = VgaCore::default();
        let extensions = registers::Extensions::default();
        let output = Output::new(
            clock,
            Self::display_timing(&vga, &extensions),
            DisplayConfiguration::vga(),
        );
        Self {
            vga,
            extensions,
            ramdac: ramdac::Sc11483::default(),
            display_start_high: 0,
            memory: VideoMemory::new(VIDEO_MEMORY_BYTES),
            output,
            io: None,
        }
    }

    pub fn video_memory_bytes(&self) -> u32 {
        self.memory.capacity()
    }

    pub fn registers(&self) -> &Registers {
        self.vga.registers()
    }

    pub fn core_state(&self) -> VgaCoreState {
        self.vga.core_state()
    }

    /// The latest pending capture for one graphics consumer.
    pub fn frames(&self) -> async_channel::Receiver<DisplaySnapshot> {
        self.output.frames.clone()
    }
}
