//! The complete DISPI adapter: shared VGA core, one VRAM, and one raster/output lifetime.
//!
//! Its banked ID2 interface exposes three resolutions in indexed eight-bit,
//! RGB565 and BGRX8888 formats, with the no-clear enable flag. Word transfers
//! at 01CE/01CF follow the [Bochs VBE Display API](https://github.com/bochs-emu/VGABIOS/blob/master/vgabios/vbe_display_api.txt).
//! Linear access, 15/24-bit modes, ID negotiation and newer flags are unsupported.
//! Invalid requests leave the accepted configuration unchanged.
mod configuration;
mod device;
mod effects;
mod framebuffer;
mod mmio;
mod modes;
mod output;
mod ports;
mod registers;
mod retrace;
mod state;

use crate::{
    memory::{VideoMemory, VramWindow},
    output::Output,
    DisplaySnapshot, Registers, Timing, VgaCore,
};
use async_channel::Receiver;
pub use modes::{DispiMode, DispiResolution};
use registers::Dispi;
pub use registers::{
    BANK, BANK_BYTES, BANK_RANGE, BANK_START, BITS_PER_PIXEL, DATA_PORT, DISABLED, ENABLE, ENABLED,
    ID, INDEX_PORT, INTERFACE_ID, NO_CLEAR_MEMORY, PORT_RANGE, VIDEO_MEMORY_BYTES, VIRTUAL_HEIGHT,
    VIRTUAL_WIDTH, X_OFFSET, X_RESOLUTION, Y_OFFSET, Y_RESOLUTION,
};
use relocade_machine::{Clock, Runner};
pub use state::DispiState;
use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};

/// All physical ports implemented by this card.
pub const PORT_RANGES: [RangeInclusive<u16>; 8] = {
    let [option, mono_crtc, mono_status, common, color_crtc, color_status, control] =
        crate::registers::PORT_RANGES;
    [
        PORT_RANGE,
        option,
        mono_crtc,
        mono_status,
        common,
        color_crtc,
        color_status,
        control,
    ]
};

/// An emulated VGA device owned by the execution worker.
/// Registers and video memory are ordinary mutable state. Other workers receive
/// immutable display snapshots through [`DispiAdapter::frames`], without accessing the live device.
/// New video memory is zeroed. The BIOS initializes its font memory, text pages and registers.
pub struct DispiAdapter {
    vga: VgaCore,
    memory: VideoMemory,
    dispi: Dispi,
    packed_window: VramWindow,
    io: Option<crate::io::Io>,
    output: Output,
}

impl DispiAdapter {
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
        let vga = VgaCore::default();
        let output = Output::new(clock, Timing::vga(&vga.registers));
        Self {
            memory: VideoMemory::new(VIDEO_MEMORY_BYTES),
            vga,
            dispi: Dispi::default(),
            packed_window: VramWindow {
                cpu_start: BANK_START,
                vram_start: 0,
                bytes: BANK_BYTES,
            },
            io: None,
            output,
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
        &self.vga.registers
    }
}
