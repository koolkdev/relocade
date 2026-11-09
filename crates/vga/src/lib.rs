//! VGA hardware state, immutable captures and GPU text/graphics decoding.
//!
//! The device owns physical VRAM, register banks and programmable raster timing.
//! BIOS and direct guest programming use the same ports and memory aperture.
//! Captures retain complete inputs sampled at retrace boundaries; historical
//! scanout within a frame and CPU-cycle timing are separate fidelity milestones.
#![forbid(unsafe_code)]

mod capture;
mod device;
#[cfg(feature = "renderer")]
mod display;
mod framebuffer;
mod geometry;
mod memory;
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

pub use capture::{CapturedFrame, FrameSource};
#[cfg(feature = "renderer")]
pub use display::VgaDisplay;
use event_listener::Event;
pub use framebuffer::{Framebuffer, FramebufferAccess, FramebufferGeometry, Pixel, TextCell};
pub use geometry::DisplayGeometry;
use memory::VideoMemory;
pub use memory::VIDEO_MEMORY_BYTES;
use raster::Raster;
use registers::{dispi, setup::Setup};
pub use registers::{Registers, CRTC_DATA_PORT, CRTC_INDEX_PORT, CRTC_PORT_RANGE, PORT_RANGES};
use relocade_machine::{Clock, DeviceConnection, Instant, Runner};
#[cfg(feature = "renderer")]
pub use renderer::{VgaImage, VgaRenderer};
pub use retrace::RetraceEvent;
pub use scanout::{Scanout, VgaDisplayState, VgaGeometry};
#[cfg(feature = "renderer")]
use snapshot::MemoryChanges;
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
/// immutable display captures through [`FrameSource`], without accessing the live device.
/// New video memory is zeroed. The BIOS initializes its font memory, text pages and registers.
pub struct Vga {
    memory: VideoMemory,
    registers: Registers,
    setup: Setup,
    dispi: dispi::Dispi,
    clock: Clock,
    origin: Instant,
    raster: Raster,
    programming_changed: Event,
    frames: FrameSource,
}

impl Vga {
    /// Creates VGA on the runner's clock and starts display capture.
    /// Returns the device handle and its memory/port connection for the CPU bus.
    pub async fn initialize(runner: &Runner) -> (Rc<RefCell<Self>>, DeviceConnection) {
        let vga = Rc::new(RefCell::new(Self::with_clock(runner.clock().clone())));
        let connection = runner
            .connect(vga.clone(), [MEMORY_RANGE], PORT_RANGES)
            .await;
        (vga, connection)
    }

    /// Uses the same timebase for guest status reads and asynchronous scanout.
    pub fn with_clock(clock: Clock) -> Self {
        let registers = Registers::default();
        let origin = clock.now();
        let raster = Raster::new(origin, Timing::vga(&registers));
        Self {
            memory: VideoMemory::default(),
            registers,
            setup: Setup::default(),
            dispi: dispi::Dispi::default(),
            clock,
            origin,
            raster,
            programming_changed: Event::new(),
            frames: FrameSource::new(),
        }
    }

    /// Read-only captured inputs for a caller-selected renderer.
    pub fn frame_source(&self) -> FrameSource {
        self.frames.clone()
    }

    /// Inspect live VGA register banks on the execution owner, including CPU
    /// port access state. Display captures retain only display inputs.
    pub fn registers(&self) -> &Registers {
        &self.registers
    }

    /// Captures current display inputs immediately on the execution owner, without
    /// advancing time, publishing a frame, or changing CPU read/access latches.
    /// Shares immutable memory and copies the active display settings and DAC.
    /// Packed SVGA captures own their layout independently of the CPU bank.
    /// The display-start latch stays at the last serviced VGA retrace; this does not
    /// force pending CRTC start-address writes to become effective.
    /// Unlike [`FrameSource`], this does not wait for a refresh boundary.
    pub fn snapshot(&self) -> DisplaySnapshot {
        let scanout = match self.dispi.display_layout() {
            Some(layout) => Scanout::Svga(layout),
            None => Scanout::Vga(VgaDisplayState::capture(
                &self.registers,
                self.raster.retrace_count,
            )),
        };
        DisplaySnapshot {
            memory: self.memory.snapshot(scanout.memory_bytes()),
            dac: DacSnapshot {
                colors: self.registers.dac.colors,
                mask: self.registers.dac.mask,
            },
            scanout,
            #[cfg(feature = "renderer")]
            changes: self.frames.current_capture().map(|previous| MemoryChanges {
                baseline: previous.display.memory.clone(),
                pages: self.memory.changed_pages(),
            }),
        }
    }

    /// Display writes only need to sample if the cached retrace deadline elapsed.
    /// Preserve the preceding registers and VRAM before applying the mutation.
    fn capture_if_due(&mut self) {
        if let Some(deadline) = self.raster.next_retrace_at() {
            let now = self.clock.now();
            if now >= deadline {
                self.advance_raster(now);
            }
        }
    }

    fn advance_raster(&mut self, now: Instant) {
        if let Some(time) = self.raster.update_to(now) {
            if self.dispi.display_layout().is_none() {
                self.registers.crtc.latch_display_start();
            }
            let display = self.snapshot();
            if self
                .frames
                .current_capture()
                .is_some_and(|previous| display.same_inputs(&previous.display))
            {
                return;
            }
            // Only publication starts a new dirty interval. Immediate snapshots
            // must leave writes visible to the next published capture.
            #[cfg(feature = "renderer")]
            self.memory.clear_changes();
            self.frames.publish(CapturedFrame {
                display,
                time: time.duration_since(self.origin),
                sequence: self.raster.retrace_count,
            });
        }
    }
}

impl Drop for Vga {
    fn drop(&mut self) {
        self.programming_changed.notify(usize::MAX);
        self.frames.close();
    }
}
