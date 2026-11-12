//! DISPI capabilities and fixed VESA DMT 60-Hz rasters.
//! https://glenwing.github.io/docs/VESA-DMT-1.13.pdf (DMT IDs 04h, 09h, 10h).
use crate::{PackedLayout, PixelFormat, Timing};

/// Supported device resolutions, each with a fixed DMT raster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispiResolution {
    R640x480,
    R800x600,
    R1024x768,
}

impl DispiResolution {
    pub const fn size(self) -> [u32; 2] {
        match self {
            Self::R640x480 => [640, 480],
            Self::R800x600 => [800, 600],
            Self::R1024x768 => [1024, 768],
        }
    }
}

/// A device resolution and pixel format. VESA BIOS numbers belong to firmware.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DispiMode {
    resolution: DispiResolution,
    format: PixelFormat,
}

impl DispiMode {
    pub const fn new(resolution: DispiResolution, format: PixelFormat) -> Self {
        Self { resolution, format }
    }

    pub fn from_registers(width: u16, height: u16, bits_per_pixel: u16) -> Option<Self> {
        let resolution = match (width, height) {
            (640, 480) => DispiResolution::R640x480,
            (800, 600) => DispiResolution::R800x600,
            (1024, 768) => DispiResolution::R1024x768,
            _ => return None,
        };
        let format = match bits_per_pixel {
            8 => PixelFormat::Indexed8,
            16 => PixelFormat::Rgb565,
            32 => PixelFormat::Bgrx8888,
            _ => return None,
        };
        Some(Self::new(resolution, format))
    }

    pub fn resolution(self) -> DispiResolution {
        self.resolution
    }

    pub fn format(self) -> PixelFormat {
        self.format
    }

    pub fn size(self) -> [u32; 2] {
        self.resolution.size()
    }

    pub fn bits_per_pixel(self) -> u8 {
        self.format.bits_per_pixel()
    }

    /// Largest whole-pixel byte stride that accommodates the visible height.
    pub fn max_pitch(self, memory_bytes: u32) -> u32 {
        let pixel_bytes = self.format.bytes_per_pixel();
        memory_bytes / self.size()[1] / pixel_bytes * pixel_bytes
    }

    pub fn layout(self, memory_bytes: u32) -> Option<PackedLayout> {
        PackedLayout::new(
            self.size(),
            self.format,
            self.size()[0] * self.format.bytes_per_pixel(),
            0,
            memory_bytes,
        )
    }

    pub(crate) fn timing(self) -> Timing {
        let (clock, total, retrace) = match self.resolution {
            DispiResolution::R640x480 => (25_175_000, [800, 525], 490..492),
            DispiResolution::R800x600 => (40_000_000, [1056, 628], 601..605),
            DispiResolution::R1024x768 => (65_000_000, [1344, 806], 771..777),
        };
        Timing::packed(clock, self.size(), total, retrace)
    }
}
