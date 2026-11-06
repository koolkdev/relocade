//! Shared SVGA mode capabilities and validated layouts.
//! DISPI, BIOS services and scanout consume these device values.
use crate::DisplayGeometry;

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

    pub fn layout(self) -> SvgaLayout {
        SvgaLayout {
            mode: self,
            pitch: self.size()[0],
            display_start: 0,
        }
    }
}

/// A supported packed indexed layout. Its private fields keep
/// the visible image within captured VRAM and the renderer's supported sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SvgaLayout {
    mode: SvgaMode,
    pitch: u32,
    display_start: u32,
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
        self.display_start
    }

    pub fn geometry(self) -> DisplayGeometry {
        DisplayGeometry {
            size: self.size(),
            pixel_aspect_ratio: (1, 1),
        }
    }

    pub(crate) fn memory_bytes(self) -> u32 {
        self.display_start + (self.size()[1] - 1) * self.pitch + self.size()[0]
    }
}
