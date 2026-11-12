//! VGA hardware state, immutable captures and GPU text/graphics decoding.
//!
//! The device owns physical VRAM, register banks and programmable raster timing.
//! BIOS and direct guest programming use the same ports and memory aperture.
//! Captures retain complete inputs sampled at retrace boundaries; historical
//! scanout within a frame and CPU-cycle timing are separate fidelity milestones.
#![forbid(unsafe_code)]

mod device;
#[cfg(feature = "renderer")]
mod display;
mod effects;
mod framebuffer;
mod geometry;
mod memory;
mod output;
mod ports;
mod raster;
pub mod registers;
#[cfg(feature = "renderer")]
mod renderer;
mod retrace;
mod scanout;
mod snapshot;
mod state;
mod svga;
mod timing;

use async_channel::Receiver;
#[cfg(feature = "renderer")]
pub use display::VgaDisplay;
use event_listener::Event;
pub use framebuffer::{Framebuffer, FramebufferAccess, FramebufferGeometry, Pixel, TextCell};
pub use geometry::DisplayGeometry;
use memory::VideoMemory;
pub use memory::VIDEO_MEMORY_BYTES;
use output::Output;
use raster::Raster;
use registers::{dispi, setup::Setup};
pub use registers::{Registers, CRTC_DATA_PORT, CRTC_INDEX_PORT, CRTC_PORT_RANGE, PORT_RANGES};
use relocade_machine::{Clock, Runner};
#[cfg(feature = "renderer")]
pub use renderer::VgaRenderer;
pub use retrace::RetraceEvent;
pub use scanout::{Scanout, VgaDisplayState, VgaGeometry};
pub use snapshot::{DacSnapshot, DisplaySnapshot};
pub use state::{DacState, DispiState, VgaCoreState};
use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};
pub use svga::{PixelFormat, SvgaLayout, SvgaMode, SvgaResolution};
use timing::Timing;

/// The color text aperture selected by the BIOS text preset.
pub const TEXT_MEMORY_START: u32 = 0xb8000;
pub const TEXT_MEMORY_BYTES: u32 = 0x8000;
pub const TEXT_MEMORY_RANGE: RangeInclusive<u32> =
    TEXT_MEMORY_START..=TEXT_MEMORY_START + TEXT_MEMORY_BYTES - 1;

/// CPU addresses decoded by VGA's programmable memory-map selector.
/// Register this entire range on both the physical memory map and the device bus.
pub const MEMORY_RANGE: RangeInclusive<u32> = 0xa0000..=0xbffff;

/// An emulated VGA device owned by the execution worker.
/// Registers and video memory are ordinary mutable state. Other workers receive
/// immutable display snapshots through [`Vga::frames`], without accessing the live device.
/// New video memory is zeroed. The BIOS initializes its font memory, text pages and registers.
pub struct Vga {
    memory: VideoMemory,
    registers: Registers,
    setup: Setup,
    dispi: dispi::Dispi,
    io: Option<ports::Io>,
    clock: Clock,
    raster: Raster,
    configuration_changed: Event,
    output: Output,
}

impl Vga {
    /// Creates VGA on the runner's clock and starts display capture.
    pub async fn initialize(runner: &Runner) -> Rc<RefCell<Self>> {
        let vga = Rc::new(RefCell::new(Self::with_clock(runner.clock().clone())));
        runner
            .connect(vga.clone(), |io| Self::configure_io(io, &vga))
            .await;
        vga
    }

    /// Uses the same timebase for guest status reads and asynchronous scanout.
    pub fn with_clock(clock: Clock) -> Self {
        let registers = Registers::default();
        let raster = Raster::new(clock.clone(), Timing::vga(&registers));
        Self {
            memory: VideoMemory::default(),
            registers,
            setup: Setup::default(),
            dispi: dispi::Dispi::default(),
            io: None,
            clock,
            raster,
            configuration_changed: Event::new(),
            output: Output::new(),
        }
    }

    /// Receives snapshots synchronized to host time. The capacity-one queue keeps
    /// the latest pending snapshot. Cloned receivers compete for the same items;
    /// use one graphics consumer and retain its last snapshot for redraws.
    pub fn frames(&self) -> Receiver<DisplaySnapshot> {
        self.output.frames.clone()
    }

    /// Inspect live VGA register banks on the execution owner, including CPU
    /// port access state. Display captures retain only display inputs.
    pub fn registers(&self) -> &Registers {
        &self.registers
    }
}

impl Drop for Vga {
    fn drop(&mut self) {
        self.configuration_changed.notify(usize::MAX);
    }
}
