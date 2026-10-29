//! VGA device state and scanout for the initial 80-by-25, 8-by-8 color text mode.
//!
//! The device owns video memory, VGA registers and fixed text-mode raster timing.
//! Its text aperture is MMIO, independent of system RAM. BIOS bookkeeping belongs to the
//! BIOS; direct port writes change hardware state without changing that data.
//! Graphics modes, font/geometry programming, and effects within a scanline
//! are not implemented yet.
#![forbid(unsafe_code)]

mod capture;
mod frame;
mod memory;
pub mod registers;
#[cfg(feature = "renderer")]
mod renderer;
mod scanout;
mod screen;
mod text;
mod timing;

pub use capture::{CapturedFrame, FrameSource};
pub use frame::VideoFrame;
use futures_lite::future::yield_now;
use memory::VideoMemory;
use registers::Registers;
pub use registers::{CRTC_DATA_PORT, CRTC_INDEX_PORT, CRTC_PORT_RANGE, PORT_RANGES};
use relocade_machine::{Clock, Device, TimerHandle};
#[cfg(feature = "renderer")]
pub use renderer::{VgaImage, VgaRenderer};
pub use screen::{TextCell, TextCursor, TextScreen};
use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};
use timing::Timing;

/// The color text aperture in the supported fixed mode.
pub const TEXT_MEMORY_START: u32 = 0xb8000;
pub const TEXT_MEMORY_BYTES: u32 = 0x8000;
pub const TEXT_MEMORY_RANGE: RangeInclusive<u32> =
    TEXT_MEMORY_START..=TEXT_MEMORY_START + TEXT_MEMORY_BYTES - 1;

/// An emulated VGA device owned by the execution worker.
/// Registers and video memory are ordinary mutable state. Other workers receive
/// immutable text captures through [`FrameSource`], without accessing the live device.
/// New video memory is zeroed. The BIOS initializes its text pages and registers.
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

    /// Copies the current device state on its owning worker. The returned screen
    /// stays stable across later device writes; it does not publish to displays.
    pub fn screen(&self) -> TextScreen {
        let crtc = &self.registers.crtc;
        TextScreen {
            cells: std::array::from_fn(|index| {
                self.memory
                    .cell(crtc.start_address().wrapping_add(index as u16))
            }),
            cursor: TextCursor {
                address: crtc.cursor_address(),
                start_scan_line: crtc.cursor_start(),
                end_scan_line: crtc.cursor_end(),
            },
            start_address: crtc.start_address(),
        }
    }

    /// Capture any completed boundary before a later guest access changes its state.
    fn advance(&mut self) {
        if self.timing.advance_to(self.clock.now()) {
            self.frames.capture(CapturedFrame {
                screen: self.screen(),
                blink_on: self.timing.blink_on(),
                blanked: self.registers.sequencer.screen_off(),
                time: self.timing.completed_at(),
                sequence: self.timing.completed_frames,
            });
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
            *byte = self.memory.read(address - TEXT_MEMORY_START + index as u32);
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
                .write(address - TEXT_MEMORY_START + index as u32, *byte);
        }
    }

    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        self.advance();
        let mut value = [0; 4];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            let port = port.wrapping_add(index as u16);
            *byte = if port == self.registers.status_port() {
                self.timing.status(self.clock.now())
            } else {
                self.registers.read_port(port)
            };
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
