use std::fmt;

/// A Wasm engine diagnostic without platform-specific error types.
#[derive(Debug)]
pub struct Error {
    operation: &'static str,
    message: String,
}

impl Error {
    pub fn new(operation: &'static str, error: impl fmt::Display) -> Self {
        Self {
            operation,
            message: format!("{error:#}"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.operation, self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
