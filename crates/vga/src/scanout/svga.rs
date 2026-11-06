//! Validated packed display layout, independent of CPU bank mapping.
use crate::DisplayGeometry;

/// A supported packed indexed layout. Only the device creates layouts, keeping
/// the visible image within captured VRAM and the renderer's supported sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SvgaLayout {
    size: [u32; 2],
    pitch: u32,
    display_start: u32,
}

impl SvgaLayout {
    pub(crate) const INDEXED_640_480: Self = Self {
        size: [640, 480],
        pitch: 640,
        display_start: 0,
    };

    pub fn size(self) -> [u32; 2] {
        self.size
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
            size: self.size,
            pixel_aspect_ratio: (1, 1),
        }
    }

    pub(super) fn memory_bytes(self) -> u32 {
        self.display_start + (self.size[1] - 1) * self.pitch + self.size[0]
    }
}
