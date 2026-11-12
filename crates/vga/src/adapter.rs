//! The complete DISPI adapter: shared VGA core, one VRAM, and one raster/output lifetime.
mod configuration;
mod device;
mod effects;
mod framebuffer;
mod output;
mod ports;
mod retrace;
mod state;

use crate::{
    dispi, memory::VideoMemory, output::Output, raster::Raster, DisplaySnapshot, Registers, Timing,
    VgaCore,
};
use async_channel::Receiver;
use event_listener::Event;
use relocade_machine::{Clock, Runner};
pub use retrace::RetraceEvent;
pub use state::DispiState;
use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};

/// All physical ports implemented by this card.
pub const PORT_RANGES: [RangeInclusive<u16>; 8] = {
    let [option, mono_crtc, mono_status, common, color_crtc, color_status, control] =
        crate::registers::PORT_RANGES;
    [
        dispi::PORT_RANGE,
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
    dispi: dispi::Dispi,
    io: Option<ports::Io>,
    clock: Clock,
    raster: Raster,
    configuration_changed: Event,
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
        let raster = Raster::new(clock.clone(), Timing::vga(&vga.registers));
        Self {
            memory: VideoMemory::default(),
            vga,
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
        &self.vga.registers
    }
}

impl Drop for DispiAdapter {
    fn drop(&mut self) {
        self.configuration_changed.notify(usize::MAX);
    }
}
