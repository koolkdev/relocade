//! VGA device state, immutable display captures, and programmable text decoding.
//!
//! The device owns video memory, VGA register banks and fixed text-mode raster timing.
//! Its programmable aperture is MMIO, independent of system RAM. BIOS bookkeeping
//! belongs to the BIOS; direct port writes change hardware without changing that data.
//! Text output follows captured fonts, geometry and colors. Graphics decoding and
//! time-resolved raster effects are not implemented yet.
#![forbid(unsafe_code)]

mod capture;
mod damage;
mod display;
mod memory;
pub mod registers;
#[cfg(feature = "renderer")]
mod renderer;
mod snapshot;
mod timing;

pub use capture::{CapturedFrame, FrameSource};
pub use damage::{MemoryDamage, MemoryRect};
pub use display::{TextDisplay, TextLayout, TextScanline};
use futures_lite::future::yield_now;
use memory::VideoMemory;
use registers::Registers;
pub use registers::{CRTC_DATA_PORT, CRTC_INDEX_PORT, CRTC_PORT_RANGE, PORT_RANGES};
use relocade_machine::{Clock, Device, TimerHandle};
#[cfg(feature = "renderer")]
pub use renderer::{VgaImage, VgaRenderer};
pub use snapshot::{DisplayRegisters, DisplaySnapshot};
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
    timing: Timing,
    frames: FrameSource,
}

impl<C: Clock> Vga<C> {
    /// Uses the same timebase for guest status reads and asynchronous scanout.
    pub fn with_clock(clock: Rc<C>) -> Self {
        let timing = Timing::new(clock.now());
        Self {
            memory: VideoMemory::default(),
            registers: Registers::default(),
            clock,
            timing,
            frames: FrameSource::new(),
        }
    }

    /// Read-only captured inputs for a caller-selected renderer.
    pub fn frame_source(&self) -> FrameSource {
        self.frames.clone()
    }

    /// Copies current display inputs immediately on the execution owner, without
    /// advancing time, publishing a frame, or changing CPU read/access latches.
    /// Unlike [`FrameSource`], this does not wait for a refresh boundary.
    pub fn snapshot(&self) -> DisplaySnapshot {
        DisplaySnapshot {
            planes: self.memory.snapshot(),
            registers: self.registers.snapshot(),
            blink_on: self.timing.blink_on(),
            changes: None,
        }
    }

    /// Capture any completed boundary before a later guest access changes its state.
    fn advance(&mut self) {
        if self.timing.advance_to(self.clock.now()) {
            let frame = CapturedFrame {
                display: self.snapshot(),
                time: self.timing.completed_at(),
                sequence: self.timing.completed_frames,
            };
            self.frames.capture(frame, self.memory.take_damage());
        }
    }
}

impl<C: Clock> Device for Vga<C> {
    /// Capture text while the CPU is busy, idle, or awaiting a service.
    /// The runner polls this activity alongside CPU execution. No device borrow
    /// spans a wait; ordinary Rust yielding handles a timer that is already late.
    async fn run(device: Rc<RefCell<Self>>) {
        let clock = device.borrow().clock.clone();
        loop {
            let deadline = {
                let mut device = device.borrow_mut();
                device.advance();
                device.timing.next_frame_at()
            };
            clock.wait_until(deadline).await;
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
        self.advance();
        if !self.registers.video_memory_enabled() {
            return;
        }
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            self.memory
                .write(address + index as u32, *byte, &self.registers);
        }
    }

    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        self.advance();
        let mut value = [0; 4];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            let port = port.wrapping_add(index as u16);
            *byte = self
                .registers
                .read_port(port, self.timing.status(self.clock.now()));
        }
        u32::from_le_bytes(value)
    }

    fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        self.advance();
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            self.registers
                .write_port(port.wrapping_add(index as u16), *byte);
        }
    }
}

impl<C> Drop for Vga<C> {
    fn drop(&mut self) {
        self.frames.close();
    }
}
