//! Shared SVGA mode capabilities and validated layouts.
//! DISPI, BIOS services and scanout consume these device values.
use crate::{DisplayGeometry, VIDEO_MEMORY_BYTES};

/// Pixel interpretation in packed VRAM, independent of resolution and CPU banking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Indexed8,
    /// Little-endian red 5, green 6, blue 5 bits.
    Rgb565,
    /// Blue, green, red, unused bytes; display output is always opaque.
    Bgrx8888,
}

impl PixelFormat {
    pub const fn bits_per_pixel(self) -> u8 {
        match self {
            Self::Indexed8 => 8,
            Self::Rgb565 => 16,
            Self::Bgrx8888 => 32,
        }
    }

    pub const fn bytes_per_pixel(self) -> u32 {
        self.bits_per_pixel() as u32 / 8
    }
}

/// Supported device resolutions, each with a fixed DMT raster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgaResolution {
    R640x480,
    R800x600,
    R1024x768,
}

impl SvgaResolution {
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
pub struct SvgaMode {
    resolution: SvgaResolution,
    format: PixelFormat,
}

impl SvgaMode {
    pub const fn new(resolution: SvgaResolution, format: PixelFormat) -> Self {
        Self { resolution, format }
    }

    pub fn from_registers(width: u16, height: u16, bits_per_pixel: u16) -> Option<Self> {
        let resolution = match (width, height) {
            (640, 480) => SvgaResolution::R640x480,
            (800, 600) => SvgaResolution::R800x600,
            (1024, 768) => SvgaResolution::R1024x768,
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

    pub fn resolution(self) -> SvgaResolution {
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
    pub fn max_pitch(self) -> u32 {
        let pixel_bytes = self.format.bytes_per_pixel();
        VIDEO_MEMORY_BYTES / self.size()[1] / pixel_bytes * pixel_bytes
    }

    pub fn layout(self) -> SvgaLayout {
        SvgaLayout {
            mode: self,
            pitch: self.size()[0] * self.format.bytes_per_pixel(),
            origin: [0, 0],
        }
    }
}

/// A packed pixel layout. Its private fields keep
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

    pub fn format(self) -> PixelFormat {
        self.mode.format()
    }

    /// Bytes between successive image rows.
    pub fn pitch(self) -> u32 {
        self.pitch
    }

    /// Physical VRAM byte containing the top-left pixel, independent of the CPU bank.
    pub fn display_start(self) -> u32 {
        self.origin[1] * self.pitch + self.origin[0] * self.format().bytes_per_pixel()
    }

    /// Top-left viewport coordinates [x, y], in pixels.
    pub fn origin(self) -> [u32; 2] {
        self.origin
    }

    pub fn virtual_height(self) -> u32 {
        VIDEO_MEMORY_BYTES / self.pitch
    }

    /// Pixels in a complete logical row, including horizontal padding.
    pub fn virtual_width(self) -> u32 {
        self.pitch / self.format().bytes_per_pixel()
    }

    /// Complete display pages, including the first page, independent of origin.
    pub fn image_pages(self) -> u32 {
        self.virtual_height() / self.size()[1]
    }

    /// Changes the byte stride while retaining the visible pixel coordinates.
    /// The full logical rows and the current viewport must still fit in VRAM.
    pub fn with_pitch(self, pitch: u32) -> Option<Self> {
        let pixel_bytes = self.format().bytes_per_pixel();
        if !pitch.is_multiple_of(pixel_bytes)
            || !(self.size()[0] * pixel_bytes..=self.mode.max_pitch()).contains(&pitch)
        {
            return None;
        }
        Self { pitch, ..self }.with_origin(self.origin)
    }

    /// Changes the logical row width in pixels, retaining the viewport origin.
    pub fn with_virtual_width(self, pixels: u32) -> Option<Self> {
        self.with_pitch(pixels.checked_mul(self.format().bytes_per_pixel())?)
    }

    /// Places the viewport within complete logical rows, without memory wrapping.
    pub fn with_origin(self, origin: [u32; 2]) -> Option<Self> {
        (origin[0] <= self.virtual_width() - self.size()[0]
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
        self.display_start()
            + (self.size()[1] - 1) * self.pitch
            + self.size()[0] * self.format().bytes_per_pixel()
    }
}
