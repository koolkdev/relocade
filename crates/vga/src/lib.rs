//! VGA registers, planar memory, programmable timing and text pixel output.
//!
//! The device owns video memory, VGA registers and programmable raster timing.
//! Its text aperture is MMIO, independent of system RAM. BIOS bookkeeping belongs to the
//! BIOS; direct port writes change hardware state without changing that data.
//! Graphics serialization and effects within a scanline are not implemented yet.
#![forbid(unsafe_code)]

mod display;
mod memory;
mod raster;
pub mod registers;
mod scanout;
mod screen;
mod timing;

pub use display::{Display, VideoFrame};
use event_listener::Event;
use futures_lite::future::{or, yield_now};
use memory::VideoMemory;
use raster::Raster;
use registers::Registers;
pub use registers::{CRTC_DATA_PORT, CRTC_INDEX_PORT, CRTC_PORT_RANGE, PORT_RANGES};
use relocade_machine::{Clock, Device, TimerHandle};
pub use screen::{TextCell, TextCursor, TextScreen};
use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};
use timing::Timing;

/// The color text aperture in the supported fixed mode.
pub const TEXT_MEMORY_START: u32 = 0xb8000;
pub const TEXT_MEMORY_BYTES: u32 = 0x8000;
pub const TEXT_MEMORY_RANGE: RangeInclusive<u32> =
    TEXT_MEMORY_START..=TEXT_MEMORY_START + TEXT_MEMORY_BYTES - 1;

/// All host addresses decoded by the VGA's programmable memory-map selector.
pub const MEMORY_RANGE: RangeInclusive<u32> = 0xa0000..=0xbffff;

/// An emulated VGA device owned by the execution worker.
/// Registers and video memory are ordinary mutable state. Other workers receive
/// completed frames through [`Display`], without accessing the live device.
/// The bootstrap text mode includes a font in plane two; other memory is zeroed.
/// BIOS mode setting initializes text pages and programs the hardware registers.
pub struct Vga<C = TimerHandle> {
    memory: VideoMemory,
    registers: Registers,
    clock: Rc<C>,
    raster: Raster,
    timing_changed: Event,
    display: Display,
}

impl<C: Clock> Vga<C> {
    /// Uses the same timebase for guest status reads and asynchronous scanout.
    pub fn with_clock(clock: Rc<C>) -> Self {
        let raster = Raster::new(clock.now());
        Self {
            memory: VideoMemory::default(),
            registers: Registers::default(),
            clock,
            raster,
            timing_changed: Event::new(),
            display: Display::new(),
        }
    }

    /// Retains completed video output independently of device ownership.
    pub fn display(&self) -> Display {
        self.display.clone()
    }

    /// Copies an 80-by-25 text-memory window from the current start address for
    /// BIOS inspection and debugging. Programmable display geometry and fonts
    /// are represented by pixel frames, not by this fixed-size memory window.
    pub fn screen(&self) -> TextScreen {
        let crtc = &self.registers.crtc;
        TextScreen {
            cells: std::array::from_fn(|index| {
                self.memory
                    .cell(crtc.display_address(crtc.start_address().wrapping_add(index as u16), 0))
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
        if let Some(boundary) = self
            .raster
            .advance(self.clock.now(), Timing::new(&self.registers))
        {
            self.display
                .complete(scanout::render(&self.memory, &self.registers, &boundary));
        }
    }
}

impl<C: Clock> Device for Vga<C> {
    /// Publish frames while the CPU is busy, idle, or awaiting a service.
    /// The runner polls this activity alongside CPU execution. No device borrow
    /// spans a wait; ordinary Rust yielding handles a timer that is already late.
    async fn run(device: Rc<RefCell<Self>>) {
        let clock = device.borrow().clock.clone();
        loop {
            let (deadline, changed) = {
                let mut device = device.borrow_mut();
                device.advance();
                (
                    device.raster.next_frame(Timing::new(&device.registers)),
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
            *byte = self
                .memory
                .read(address.wrapping_add(index as u32), &self.registers);
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
                .write(address.wrapping_add(index as u32), *byte, &self.registers);
        }
    }

    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        self.advance();
        let mut value = [0; 4];
        let raster_status = self.raster.status(Timing::new(&self.registers));
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            let port = port.wrapping_add(index as u16);
            *byte = self.registers.read_port(port, raster_status);
        }
        u32::from_le_bytes(value)
    }

    fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        self.advance();
        let old = Timing::new(&self.registers);
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            self.registers
                .write_port(port.wrapping_add(index as u16), *byte);
        }
        let new = Timing::new(&self.registers);
        if old != new {
            self.raster.reconfigure(old, new);
            self.timing_changed.notify(usize::MAX);
        }
    }
}

impl<C> Drop for Vga<C> {
    fn drop(&mut self) {
        self.display.close();
    }
}
