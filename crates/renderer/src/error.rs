use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrameError {
    /// A dimension is zero, exceeds device limits, or overflows the byte count.
    InvalidSize,
    /// Pixel aspect ratio must have a nonzero numerator and denominator.
    InvalidAspectRatio,
    /// The frame does not contain exactly four bytes per pixel.
    InvalidLength { expected: usize, actual: usize },
    /// The renderer supports SDR RGBA8 and BGRA8 targets, with or without sRGB encoding.
    UnsupportedFormat(wgpu::TextureFormat),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSize => write!(f, "frame dimensions are empty or exceed device limits"),
            Self::InvalidAspectRatio => write!(f, "pixel aspect ratio must be nonzero"),
            Self::InvalidLength { expected, actual } => {
                write!(f, "RGBA frame requires {expected} bytes, received {actual}")
            }
            Self::UnsupportedFormat(format) => {
                write!(f, "unsupported pixel target format: {format:?}")
            }
        }
    }
}

impl std::error::Error for FrameError {}
