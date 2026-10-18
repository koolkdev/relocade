use std::fmt;
use wasm86_compiler::BuildError;
use wasm86_machine::ConfigurationError;

/// Host setup/execution errors, separate from architectural guest exits.
#[derive(Debug)]
pub enum Error {
    Generate(BuildError),
    Configuration(ConfigurationError),
    Wasm {
        operation: &'static str,
        message: String,
    },
    InvalidExit(u64),
}

impl Error {
    /// Retains an engine diagnostic without exposing platform-specific types.
    pub fn wasm(operation: &'static str, error: impl fmt::Display) -> Self {
        Self::Wasm {
            operation,
            message: format!("{error:#}"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generate(error) => write!(f, "generate execution module: {error}"),
            Self::Configuration(error) => error.fmt(f),
            Self::Wasm { operation, message } => write!(f, "{operation}: {message}"),
            Self::InvalidExit(word) => write!(f, "invalid guest exit word {word:#018x}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Generate(error) => Some(error),
            Self::Configuration(error) => Some(error),
            _ => None,
        }
    }
}

impl From<BuildError> for Error {
    fn from(error: BuildError) -> Self {
        Self::Generate(error)
    }
}

impl From<ConfigurationError> for Error {
    fn from(error: ConfigurationError) -> Self {
        Self::Configuration(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
