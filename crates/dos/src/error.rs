use std::fmt;

/// Error numbers returned to the guest in AX with carry set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DosError {
    InvalidFunction,
    FileNotFound,
    PathNotFound,
    TooManyOpenFiles,
    AccessDenied,
    InvalidHandle,
    MemoryCorrupted,
    InsufficientMemory { available: u16 },
    InvalidMemoryBlock,
    InvalidAccess,
    InvalidDrive,
    NoMoreFiles,
}

impl DosError {
    pub(crate) fn code(self) -> u16 {
        match self {
            Self::InvalidFunction => 1,
            Self::FileNotFound => 2,
            Self::PathNotFound => 3,
            Self::TooManyOpenFiles => 4,
            Self::AccessDenied => 5,
            Self::InvalidHandle => 6,
            Self::MemoryCorrupted => 7,
            Self::InsufficientMemory { .. } => 8,
            Self::InvalidMemoryBlock => 9,
            Self::InvalidAccess => 12,
            Self::InvalidDrive => 15,
            Self::NoMoreFiles => 18,
        }
    }
}

/// A host-supplied file could not be mounted in the DOS environment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MountError {
    InvalidPath,
    FileTooLarge,
}

impl fmt::Display for MountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidPath => "expected a nonempty ASCII file path on C: shorter than 128 bytes",
            Self::FileTooLarge => "mounted file exceeds the 32-bit DOS file size",
        })
    }
}

impl std::error::Error for MountError {}

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
