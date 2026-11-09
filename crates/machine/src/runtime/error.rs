use crate::{ConfigurationError, MemoryError};
use relocade_compiler::BuildError;
use std::fmt;

/// Host setup/execution errors, separate from architectural guest exits.
#[derive(Debug)]
pub enum Error {
    Generate(BuildError),
    Compile(relocade_x86::BlockError),
    Worker(String),
    Configuration(ConfigurationError),
    Memory(MemoryError),
    Wasm(relocade_wasm_runtime::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generate(error) => write!(f, "generate execution module: {error}"),
            Self::Compile(error) => write!(f, "compile guest code: {error}"),
            Self::Worker(error) => write!(f, "worker: {error}"),
            Self::Configuration(error) => error.fmt(f),
            Self::Memory(error) => error.fmt(f),
            Self::Wasm(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Generate(error) => Some(error),
            Self::Compile(error) => Some(error),
            Self::Worker(_) => None,
            Self::Configuration(error) => Some(error),
            Self::Memory(error) => Some(error),
            Self::Wasm(error) => Some(error),
        }
    }
}

impl From<BuildError> for Error {
    fn from(error: BuildError) -> Self {
        Self::Generate(error)
    }
}

impl From<relocade_x86::BlockError> for Error {
    fn from(error: relocade_x86::BlockError) -> Self {
        Self::Compile(error)
    }
}

impl From<ConfigurationError> for Error {
    fn from(error: ConfigurationError) -> Self {
        Self::Configuration(error)
    }
}

impl From<relocade_wasm_runtime::Error> for Error {
    fn from(error: relocade_wasm_runtime::Error) -> Self {
        Self::Wasm(error)
    }
}

impl From<MemoryError> for Error {
    fn from(error: MemoryError) -> Self {
        Self::Memory(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
