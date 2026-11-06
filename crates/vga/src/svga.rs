//! Shared SVGA mode capabilities and validated layouts.
//! DISPI, BIOS services and scanout consume these device values.
use crate::{DisplayGeometry, VIDEO_MEMORY_BYTES};

/// Supported device modes. VESA BIOS numbers belong to firmware.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgaMode {
    Indexed640x480,
    Indexed800x600,
    Indexed1024x768,
}

impl SvgaMode {
    pub fn from_registers(width: u16, height: u16, bits_per_pixel: u16) -> Option<Self> {
        match (width, height, bits_per_pixel) {
            (640, 480, 8) => Some(Self::Indexed640x480),
            (800, 600, 8) => Some(Self::Indexed800x600),
            (1024, 768, 8) => Some(Self::Indexed1024x768),
            _ => None,
        }
    }

    pub fn size(self) -> [u32; 2] {
        match self {
            Self::Indexed640x480 => [640, 480],
            Self::Indexed800x600 => [800, 600],
            Self::Indexed1024x768 => [1024, 768],
        }
    }

    pub fn bits_per_pixel(self) -> u8 {
        8
    }

    /// Largest byte stride that still accommodates the visible height.
    pub fn max_pitch(self) -> u32 {
        crate::VIDEO_MEMORY_BYTES / self.size()[1]
    }

    pub fn layout(self) -> SvgaLayout {
        SvgaLayout {
            mode: self,
            pitch: self.size()[0],
            origin: [0, 0],
        }
    }
}

/// A supported packed indexed layout. Its private fields keep
/// the visible image within captured VRAM and the renderer's supported sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SvgaLayout {
    mode: SvgaMode,
    pitch: u32,
    origin: [u32; 2],
}

impl SvgaLayout {
    pub fn mode(self) -> SvgaMode {
        self.mode
    }

    pub fn size(self) -> [u32; 2] {
        self.mode.size()
    }

    /// Bytes between successive image rows.
    pub fn pitch(self) -> u32 {
        self.pitch
    }

    /// Physical VRAM byte containing the top-left pixel, independent of the CPU bank.
    pub fn display_start(self) -> u32 {
        self.origin[1] * self.pitch + self.origin[0]
    }

    /// Top-left viewport coordinates [x, y], in pixels.
    pub fn origin(self) -> [u32; 2] {
        self.origin
    }

    pub fn virtual_height(self) -> u32 {
        VIDEO_MEMORY_BYTES / self.pitch
    }

    /// Complete display pages, including the first page, independent of origin.
    pub fn image_pages(self) -> u32 {
        self.virtual_height() / self.size()[1]
    }

    /// Changes the byte stride while retaining the visible pixel coordinates.
    /// The full logical rows and the current viewport must still fit in VRAM.
    pub fn with_pitch(self, pitch: u32) -> Option<Self> {
        if !(self.size()[0]..=self.mode.max_pitch()).contains(&pitch) {
            return None;
        }
        Self { pitch, ..self }.with_origin(self.origin)
    }

    /// Places the viewport within complete logical rows, without memory wrapping.
    pub fn with_origin(self, origin: [u32; 2]) -> Option<Self> {
        (origin[0] <= self.pitch - self.size()[0]
            && origin[1] <= self.virtual_height() - self.size()[1])
            .then_some(Self { origin, ..self })
    }

    pub fn geometry(self) -> DisplayGeometry {
        DisplayGeometry {
            size: self.size(),
            pixel_aspect_ratio: (1, 1),
        }
    }

    pub(crate) fn memory_bytes(self) -> u32 {
        self.display_start() + (self.size()[1] - 1) * self.pitch + self.size()[0]
    }
}
