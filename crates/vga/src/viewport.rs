//! Bounded logical images used by firmware, independent of physical scanout.
//! Guest register programming can describe wrapping or overlapping images;
//! viewport mutations require the visible rectangle to fit in complete rows.
mod crtc;

use crate::{PackedLayout, PixelFormat};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Addressing {
    Text([u32; 2]),
    Planar,
    Indexed256,
    Packed(PixelFormat),
}

impl Addressing {
    fn address_pixels(self) -> [u32; 2] {
        match self {
            Self::Text(cell) => cell,
            Self::Planar => [8, 1],
            Self::Indexed256 => [4, 1],
            Self::Packed(_) => [1, 1],
        }
    }

    fn address_bytes(self) -> u32 {
        match self {
            Self::Text(_) => 2,
            Self::Planar => 1,
            Self::Indexed256 => 4,
            Self::Packed(format) => format.bytes_per_pixel(),
        }
    }
}

/// The programmed logical image and the card's limits for changing it.
/// Pitch counts CPU bytes (per plane in planar modes); coordinates count pixels,
/// including character dots and glyph scanlines in text modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    size: [u32; 2],
    addressing: Addressing,
    memory_bytes: u32,
    pitch_granularity: u32,
    pitch_limit: u32,
    pitch: u32,
    origin: [u32; 2],
}

impl Viewport {
    pub(crate) fn packed(
        layout: PackedLayout,
        memory_bytes: u32,
        pitch_granularity: u32,
        pitch_limit: u32,
    ) -> Self {
        Self {
            size: layout.size(),
            addressing: Addressing::Packed(layout.format()),
            memory_bytes,
            pitch_granularity,
            pitch_limit,
            pitch: layout.pitch(),
            origin: layout.origin(),
        }
    }

    pub fn pitch(self) -> u32 {
        self.pitch
    }

    pub fn virtual_width(self) -> u32 {
        self.pitch / self.addressing.address_bytes() * self.addressing.address_pixels()[0]
    }

    /// Text heights exclude hardware double scanning. A zero programmed stride
    /// can be queried, but cannot hold a bounded viewport.
    pub fn virtual_height(self) -> u32 {
        self.memory_bytes.checked_div(self.pitch).unwrap_or(0) * self.addressing.address_pixels()[1]
    }

    pub fn origin(self) -> [u32; 2] {
        self.origin
    }

    pub fn pitch_for_width(self, pixels: u32) -> u32 {
        pixels
            .max(self.size[0])
            .div_ceil(self.addressing.address_pixels()[0])
            * self.addressing.address_bytes()
    }

    pub fn max_pitch(self) -> u32 {
        let rows = self.size[1].div_ceil(self.addressing.address_pixels()[1]);
        (self.memory_bytes / rows).min(self.pitch_limit) / self.pitch_granularity
            * self.pitch_granularity
    }

    /// Round to the card's pitch unit without changing the pixel origin.
    pub fn with_pitch(mut self, pitch: u32) -> Option<Self> {
        self.pitch = pitch
            .max(self.pitch_for_width(0))
            .div_ceil(self.pitch_granularity)
            .checked_mul(self.pitch_granularity)?;
        (self.pitch <= self.max_pitch())
            .then_some(self)?
            .with_origin(self.origin)
    }

    /// Maximum capability is independent of the current viewport origin.
    pub fn maximum(mut self) -> Option<Self> {
        self.origin = [0, 0];
        self.with_pitch(self.max_pitch())
    }

    pub fn with_origin(mut self, origin: [u32; 2]) -> Option<Self> {
        if origin[0].checked_add(self.size[0])? > self.virtual_width()
            || origin[1].checked_add(self.size[1])? > self.virtual_height()
        {
            return None;
        }
        self.origin = origin;
        Some(self)
    }
}
