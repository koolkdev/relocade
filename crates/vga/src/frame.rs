use std::time::Duration;

/// Completed VGA output. Pixels are tightly packed, top-to-bottom RGBA8,
/// with sRGB color values and opaque alpha. Published frames are immutable.
#[derive(Debug)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Box<[u8]>,
    /// Pixel width / height. Combine with the image dimensions when sizing a viewport.
    pub pixel_aspect_ratio: (u32, u32),
    /// Elapsed session time at the completed frame boundary.
    pub time: Duration,
    /// Increasing frame identity; skipped frames can leave gaps.
    pub sequence: u64,
}
