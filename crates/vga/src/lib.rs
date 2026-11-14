//! VGA hardware state, immutable captures and GPU text/graphics decoding.
//!
//! The device owns physical VRAM, register banks and programmable raster timing.
//! BIOS and direct guest programming use the same ports and memory aperture.
//! Captures retain complete inputs sampled at retrace boundaries; historical
//! scanout within a frame and CPU-cycle timing are separate fidelity milestones.
#![forbid(unsafe_code)]

mod core;
pub mod dispi;
#[cfg(feature = "renderer")]
mod display;
mod effects;
pub mod et4000;
mod framebuffer;
mod geometry;
mod io;
mod legacy;
mod memory;
mod mmio;
mod output;
mod packed;
mod ports;
mod raster;
pub mod registers;
#[cfg(feature = "renderer")]
mod renderer;
mod scanout;
mod snapshot;
mod timing;

pub use core::{DacState, VgaCore, VgaCoreState};
pub use dispi::{
    DispiAdapter, DispiMode, DispiResolution, DispiState, PORT_RANGES, VIDEO_MEMORY_BYTES,
};
#[cfg(feature = "renderer")]
pub use display::VgaDisplay;
pub use et4000::{Et4000Ax, Et4000State};
pub use framebuffer::{Framebuffer, FramebufferAccess, FramebufferGeometry, Pixel, TextCell};
pub use geometry::DisplayGeometry;
pub use legacy::LegacyLayout;
pub use output::RetraceEvent;
pub use packed::{PackedLayout, PixelFormat};
pub use registers::{Registers, CRTC_DATA_PORT, CRTC_INDEX_PORT, CRTC_PORT_RANGE};
#[cfg(feature = "renderer")]
pub use renderer::VgaRenderer;
pub use scanout::{Scanout, VgaDisplayState, VgaGeometry};
pub use snapshot::{DacSnapshot, DisplaySnapshot};
use std::ops::RangeInclusive;
use timing::Timing;

/// The color text aperture selected by the BIOS text preset.
pub const TEXT_MEMORY_START: u32 = 0xb8000;
pub const TEXT_MEMORY_BYTES: u32 = 0x8000;
pub const TEXT_MEMORY_RANGE: RangeInclusive<u32> =
    TEXT_MEMORY_START..=TEXT_MEMORY_START + TEXT_MEMORY_BYTES - 1;

/// CPU addresses decoded by VGA's programmable memory-map selector.
/// Register this entire range on both the physical memory map and the device bus.
pub const MEMORY_RANGE: RangeInclusive<u32> = 0xa0000..=0xbffff;
