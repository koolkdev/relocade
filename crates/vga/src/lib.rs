//! VGA hardware state, immutable captures and GPU text/graphics decoding.
//!
//! The device owns physical VRAM, register banks and programmable raster timing.
//! BIOS and direct guest programming use the same ports and memory aperture.
//! Captures retain complete inputs sampled at retrace boundaries; historical
//! scanout within a frame and CPU-cycle timing are separate fidelity milestones.
#![forbid(unsafe_code)]

mod capture;
#[cfg(feature = "renderer")]
mod display;
mod geometry;
mod memory;
mod raster;
pub mod registers;
#[cfg(feature = "renderer")]
mod renderer;
mod snapshot;
mod timing;

pub use capture::{CapturedFrame, FrameSource};
#[cfg(feature = "renderer")]
pub use display::VgaDisplay;
use event_listener::Event;
use futures_lite::future::{or, yield_now};
pub use geometry::DisplayGeometry;
use memory::VideoMemory;
use raster::Raster;
pub use registers::{Registers, CRTC_DATA_PORT, CRTC_INDEX_PORT, CRTC_PORT_RANGE, PORT_RANGES};
use relocade_machine::{Clock, Device, TimerHandle};
#[cfg(feature = "renderer")]
pub use renderer::{VgaImage, VgaRenderer};
pub use snapshot::DisplaySnapshot;
#[cfg(feature = "renderer")]
use snapshot::MemoryChanges;
use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};
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
pub struct Vga<C = TimerHandle> {
    memory: VideoMemory,
    registers: Registers,
    clock: Rc<C>,
    raster: Raster,
    timing_changed: Event,
    frames: FrameSource,
}

impl<C: Clock> Vga<C> {
    /// Uses the same timebase for guest status reads and asynchronous scanout.
    pub fn with_clock(clock: Rc<C>) -> Self {
        let registers = Registers::default();
        let raster = Raster::new(clock.now(), Timing::new(&registers));
        Self {
            memory: VideoMemory::default(),
            registers,
            clock,
            raster,
            timing_changed: Event::new(),
            frames: FrameSource::new(),
        }
    }

    /// Read-only captured inputs for a caller-selected renderer.
    pub fn frame_source(&self) -> FrameSource {
        self.frames.clone()
    }

    /// Captures current display inputs immediately on the execution owner, without
    /// advancing time, publishing a frame, or changing CPU read/access latches.
    /// Shares immutable VRAM planes and copies the small register state.
    /// The display-start latch stays at the last serviced retrace; this does not
    /// force pending CRTC start-address writes to become effective.
    /// Unlike [`FrameSource`], this does not wait for a refresh boundary.
    pub fn snapshot(&self) -> DisplaySnapshot {
        DisplaySnapshot {
            planes: self.memory.snapshot(),
            registers: self.registers.clone(),
            blink_on: self.raster.blink_on(),
            #[cfg(feature = "renderer")]
            changes: self.frames.current_capture().map(|previous| MemoryChanges {
                baseline: previous.display.planes.clone(),
                pages: self.memory.changed_pages(),
            }),
        }
    }

    /// Bring raster state up to the shared clock and publish the latest due capture.
    /// Guest writes call this first, while the preceding registers and VRAM still apply.
    fn sync_to_clock(&mut self) {
        if let Some(time) = self.raster.update_to(self.clock.now()) {
            self.registers.crtc.latch_display_start();
            let display = self.snapshot();
            // Only publication starts a new dirty interval. Immediate snapshots
            // must leave writes visible to the next published capture.
            #[cfg(feature = "renderer")]
            self.memory.clear_changes();
            self.frames.publish(CapturedFrame {
                display,
                time,
                sequence: self.raster.retrace_count,
            });
        }
    }
}

impl<C: Clock> Device for Vga<C> {
    /// Capture display inputs while the CPU is busy, idle, or awaiting a service.
    /// The runner polls this activity alongside CPU execution. No device borrow
    /// spans a wait; ordinary Rust yielding handles a timer that is already late.
    async fn run(device: Rc<RefCell<Self>>) {
        let clock = device.borrow().clock.clone();
        loop {
            let (deadline, changed) = {
                let mut device = device.borrow_mut();
                device.sync_to_clock();
                (
                    device.raster.next_retrace_at(),
                    device.timing_changed.listen(),
                )
            };
            if let Some(deadline) = deadline {
                or(clock.wait_until(deadline), changed).await;
            } else {
                changed.await;
            }
            yield_now().await;
        }
    }

    fn read_mmio(&mut self, address: u32, bytes: u8) -> u64 {
        let mut value = [0; 8];
        if !self.registers.video_memory_enabled() {
            value[..usize::from(bytes)].fill(0xff);
            return u64::from_le_bytes(value);
        }
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            *byte = self.memory.read(address + index as u32, &self.registers);
        }
        u64::from_le_bytes(value)
    }

    fn write_mmio(&mut self, address: u32, bytes: u8, value: u64) {
        self.sync_to_clock();
        if !self.registers.video_memory_enabled() {
            return;
        }
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            self.memory
                .write(address + index as u32, *byte, &self.registers);
        }
    }

    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        self.sync_to_clock();
        let mut value = [0; 4];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            let port = port.wrapping_add(index as u16);
            *byte = self.registers.read_port(port, self.raster.input_status_1());
        }
        u32::from_le_bytes(value)
    }

    fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        self.sync_to_clock();
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            self.registers
                .write_port(port.wrapping_add(index as u16), *byte);
        }
        if self.raster.set_timing(Timing::new(&self.registers)) {
            self.timing_changed.notify(usize::MAX);
        }
    }
}

impl<C> Drop for Vga<C> {
    fn drop(&mut self) {
        self.frames.close();
    }
}
