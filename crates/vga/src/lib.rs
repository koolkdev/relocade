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
mod scanout;
mod snapshot;
mod svga;
mod timing;

pub use capture::{CapturedFrame, FrameSource};
#[cfg(feature = "renderer")]
pub use display::VgaDisplay;
use event_listener::Event;
use futures_lite::future::{or, yield_now};
pub use geometry::DisplayGeometry;
pub use memory::VIDEO_MEMORY_BYTES;
use memory::{VideoMemory, VramAccess};
use raster::Raster;
use registers::{dispi, setup::Setup, ADAPTER_CONTROL_PORT, OPTION_SELECT_PORT};
pub use registers::{Registers, CRTC_DATA_PORT, CRTC_INDEX_PORT, CRTC_PORT_RANGE, PORT_RANGES};
use relocade_machine::{Clock, Device, DeviceConnection, Runner, TimerHandle};
#[cfg(feature = "renderer")]
pub use renderer::{VgaImage, VgaRenderer};
pub use scanout::{Scanout, VgaDisplayState, VgaGeometry};
#[cfg(feature = "renderer")]
use snapshot::MemoryChanges;
pub use snapshot::{DacSnapshot, DisplaySnapshot};
use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};
pub use svga::{SvgaLayout, SvgaMode};
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
    setup: Setup,
    dispi: dispi::Dispi,
    clock: Rc<C>,
    raster: Raster,
    timing_changed: Event,
    frames: FrameSource,
}

impl<C: Clock> Vga<C> {
    /// Creates VGA on the runner's clock and starts display capture.
    /// Returns the device handle and its memory/port connection for the CPU bus.
    pub async fn initialize(runner: &Runner<C>) -> (Rc<RefCell<Self>>, DeviceConnection)
    where
        C: Clone + 'static,
    {
        let vga = Rc::new(RefCell::new(Self::with_clock(Rc::new(
            runner.clock().clone(),
        ))));
        let connection = runner
            .connect(vga.clone(), [MEMORY_RANGE], PORT_RANGES)
            .await;
        (vga, connection)
    }

    /// Uses the same timebase for guest status reads and asynchronous scanout.
    pub fn with_clock(clock: Rc<C>) -> Self {
        let registers = Registers::default();
        let raster = Raster::new(clock.now(), Timing::vga(&registers));
        Self {
            memory: VideoMemory::default(),
            registers,
            setup: Setup::default(),
            dispi: dispi::Dispi::default(),
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

    /// Bring raster state up to the shared clock and publish changed display inputs.
    /// Guest writes call this first, while the preceding registers and VRAM still apply.
    fn sync_to_clock(&mut self) {
        if let Some(time) = self.raster.update_to(self.clock.now()) {
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
        let access = VramAccess::new(&self.setup, &self.registers, &self.dispi);
        self.memory.read(address, bytes, access)
    }

    fn write_mmio(&mut self, address: u32, bytes: u8, value: u64) {
        self.sync_to_clock();
        let access = VramAccess::new(&self.setup, &self.registers, &self.dispi);
        self.memory.write(address, bytes, value, access);
    }

    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        self.sync_to_clock();
        if dispi::PORT_RANGE.contains(&port) {
            return if self.setup.enabled() {
                self.dispi.read_port(port, bytes)
            } else {
                u32::MAX >> ((4 - bytes) * 8)
            };
        }
        let mut value = [0; 4];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            let port = port.wrapping_add(index as u16);
            *byte = match port {
                OPTION_SELECT_PORT => self.setup.read_option(),
                port if self.setup.enabled() => {
                    self.registers.read_port(port, self.raster.input_status_1())
                }
                _ => 0xff,
            };
        }
        u32::from_le_bytes(value)
    }

    fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        self.sync_to_clock();
        let previous_layout = self.dispi.display_layout();
        if dispi::PORT_RANGE.contains(&port) {
            if self.setup.enabled() {
                match self.dispi.write_port(port, bytes, value) {
                    dispi::WriteEffect::None => {}
                    dispi::WriteEffect::ClearVram => self.memory.clear_vram(),
                }
            }
        } else {
            for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
                match port.wrapping_add(index as u16) {
                    ADAPTER_CONTROL_PORT => self.setup.write_control(*byte),
                    OPTION_SELECT_PORT => self.setup.write_option(*byte),
                    port if self.setup.enabled() => self.registers.write_port(port, *byte),
                    _ => {}
                }
            }
        }
        let layout = self.dispi.display_layout();
        let timing = match layout {
            Some(layout) => Timing::svga(layout),
            None => Timing::vga(&self.registers),
        };
        let timing_changed =
            if layout.map(SvgaLayout::mode) != previous_layout.map(SvgaLayout::mode) {
                self.raster.restart(timing);
                true
            } else {
                self.raster.set_timing(timing)
            };
        if timing_changed {
            self.timing_changed.notify(usize::MAX);
        }
    }
}

impl<C> Drop for Vga<C> {
    fn drop(&mut self) {
        self.frames.close();
    }
}
