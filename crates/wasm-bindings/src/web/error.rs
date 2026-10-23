use std::fmt;

/// A failure to compile, link or resolve a typed Wasm binding.
#[derive(Debug)]
pub struct Error(String);

impl Error {
    pub(crate) fn new(operation: &str, message: String) -> Self {
        Self(format!("{operation}: {message}"))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
