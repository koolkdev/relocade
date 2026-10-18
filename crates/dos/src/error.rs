use std::fmt;

/// Error numbers returned to the guest in AX with carry set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DosError {
    InvalidHandle = 6,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadError {
    EmptyImage,
    ImageTooLarge,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyImage => f.write_str("COM image is empty"),
            Self::ImageTooLarge => f.write_str("COM image exceeds 65,278 bytes"),
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
