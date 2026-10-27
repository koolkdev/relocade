use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    /// A dimension is zero, exceeds device limits, or overflows the byte count.
    InvalidSize,
    /// The upload does not contain exactly four bytes per pixel.
    InvalidLength { expected: usize, actual: usize },
    /// The renderer supports SDR RGBA8 and BGRA8 targets, with or without sRGB encoding.
    UnsupportedFormat(wgpu::TextureFormat),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSize => write!(f, "texture dimensions are empty or exceed device limits"),
            Self::InvalidLength { expected, actual } => {
                write!(
                    f,
                    "RGBA texture requires {expected} bytes, received {actual}"
                )
            }
            Self::UnsupportedFormat(format) => {
                write!(f, "unsupported pixel target format: {format:?}")
            }
        }
    }
}

impl std::error::Error for Error {}
