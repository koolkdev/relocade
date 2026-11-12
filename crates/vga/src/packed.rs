//! Packed image geometry, independent of a card's modes and raster timing.
use crate::DisplayGeometry;

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

/// A checked viewport into complete logical rows of physical video memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedLayout {
    size: [u32; 2],
    format: PixelFormat,
    pitch: u32,
    display_start: u32,
    memory_bytes: u32,
}

impl PackedLayout {
    /// Validate dimensions, pixel alignment and the viewport against actual VRAM.
    pub fn new(
        size: [u32; 2],
        format: PixelFormat,
        pitch: u32,
        display_start: u32,
        memory_bytes: u32,
    ) -> Option<Self> {
        let pixel_bytes = format.bytes_per_pixel();
        let row_bytes = size[0].checked_mul(pixel_bytes)?;
        if size.contains(&0)
            || pitch == 0
            || !pitch.is_multiple_of(pixel_bytes)
            || !display_start.is_multiple_of(pixel_bytes)
            || display_start.checked_rem(pitch)?.checked_add(row_bytes)? > pitch
            || (display_start / pitch).checked_add(size[1])? > memory_bytes / pitch
        {
            return None;
        }
        Some(Self {
            size,
            format,
            pitch,
            display_start,
            memory_bytes,
        })
    }

    pub fn size(self) -> [u32; 2] {
        self.size
    }
    pub fn format(self) -> PixelFormat {
        self.format
    }
    /// Bytes between successive image rows.
    pub fn pitch(self) -> u32 {
        self.pitch
    }
    /// Physical byte address of the top-left pixel, independent of CPU banking.
    pub fn display_start(self) -> u32 {
        self.display_start
    }
    pub fn origin(self) -> [u32; 2] {
        [
            self.display_start % self.pitch / self.format.bytes_per_pixel(),
            self.display_start / self.pitch,
        ]
    }
    pub fn virtual_width(self) -> u32 {
        self.pitch / self.format.bytes_per_pixel()
    }
    pub fn virtual_height(self) -> u32 {
        self.memory_bytes / self.pitch
    }
    pub fn image_pages(self) -> u32 {
        self.virtual_height() / self.size[1]
    }
    pub fn max_pitch(self) -> u32 {
        let pixel_bytes = self.format.bytes_per_pixel();
        self.memory_bytes / self.size[1] / pixel_bytes * pixel_bytes
    }
    /// Change pitch while retaining the viewport's pixel coordinates.
    pub fn with_pitch(self, pitch: u32) -> Option<Self> {
        Self::new(self.size, self.format, pitch, 0, self.memory_bytes)?.with_origin(self.origin())
    }
    pub fn with_virtual_width(self, pixels: u32) -> Option<Self> {
        self.with_pitch(pixels.checked_mul(self.format.bytes_per_pixel())?)
    }
    pub fn with_origin(self, [x, y]: [u32; 2]) -> Option<Self> {
        if x.checked_add(self.size[0])? > self.virtual_width() {
            return None;
        }
        let start = y
            .checked_mul(self.pitch)?
            .checked_add(x.checked_mul(self.format.bytes_per_pixel())?)?;
        Self::new(self.size, self.format, self.pitch, start, self.memory_bytes)
    }
    pub fn geometry(self) -> DisplayGeometry {
        DisplayGeometry {
            size: self.size,
            pixel_aspect_ratio: (1, 1),
        }
    }
    pub(crate) fn memory_bytes(self) -> u32 {
        self.display_start
            + (self.size[1] - 1) * self.pitch
            + self.size[0] * self.format.bytes_per_pixel()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn arbitrary_image_uses_the_supplied_memory_capacity() {
        let layout = PackedLayout::new([320, 200], PixelFormat::Rgb565, 640, 1280, 129_280)
            .expect("a packed image does not require a DISPI mode");
        assert_eq!(layout.size(), [320, 200]);
        assert_eq!(layout.origin(), [0, 2]);
        assert_eq!(layout.memory_bytes(), 129_280);
        assert!(layout.with_origin([0, 3]).is_none());
        assert!(PackedLayout::new([320, 200], PixelFormat::Rgb565, 640, 1280, 129_279).is_none());
        assert!(
            PackedLayout::new([u32::MAX, 2], PixelFormat::Bgrx8888, 640, 0, u32::MAX).is_none()
        );
        let panned = PackedLayout::new([1, 1], PixelFormat::Indexed8, 64, 40, 128).unwrap();
        assert!(
            panned.with_pitch(16).is_none(),
            "x must not wrap into another row"
        );
    }
}
