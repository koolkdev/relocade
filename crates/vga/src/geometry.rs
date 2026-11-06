//! Output geometry shared by display decoders and presentation.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayGeometry {
    pub size: [u32; 2],
    /// Pixel width / height, chosen by the active display interpretation.
    pub pixel_aspect_ratio: (u32, u32),
}

impl DisplayGeometry {
    pub fn pixel_aspect_ratio(&self) -> (u32, u32) {
        self.pixel_aspect_ratio
    }
}
