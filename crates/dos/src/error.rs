use std::fmt;

/// Error numbers returned to the guest in AX with carry set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DosError {
    InvalidHandle,
    MemoryCorrupted,
    InsufficientMemory { available: u16 },
    InvalidMemoryBlock,
}

impl DosError {
    pub(crate) fn code(self) -> u16 {
        match self {
            Self::InvalidHandle => 6,
            Self::MemoryCorrupted => 7,
            Self::InsufficientMemory { .. } => 8,
            Self::InvalidMemoryBlock => 9,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadError {
    EmptyImage,
    ImageTooLarge,
    InvalidMzHeader,
    InvalidRelocation,
    InsufficientMemory,
    UnsupportedOverlay,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyImage => f.write_str("COM image is empty"),
            Self::ImageTooLarge => f.write_str("COM image exceeds 65,278 bytes"),
            Self::InvalidMzHeader => f.write_str("invalid or truncated DOS MZ executable"),
            Self::InvalidRelocation => f.write_str("invalid MZ relocation table or target"),
            Self::InsufficientMemory => {
                f.write_str("MZ executable requires more conventional memory than available")
            }
            Self::UnsupportedOverlay => f.write_str("loading MZ overlays is not supported"),
        }
    }
}

impl std::error::Error for LoadError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceError {
    UnsupportedFunction(u8),
    UnterminatedString,
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedFunction(function) => {
                write!(f, "unsupported INT 21h function {function:02x}")
            }
            Self::UnterminatedString => {
                f.write_str("DOS string has no '$' terminator within its segment")
            }
        }
    }
}

impl std::error::Error for ServiceError {}
