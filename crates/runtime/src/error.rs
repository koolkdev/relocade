use std::fmt;
use wasm86_compiler::BuildError;

/// Host setup and execution failures, distinct from architectural guest exits.
#[derive(Debug)]
pub enum Error {
    Generate(BuildError),
    Backend {
        operation: &'static str,
        message: String,
    },
    Configuration(wasm86_machine::ConfigurationError),
    InvalidExit(u64),
}

impl Error {
    /// Preserves an engine's diagnostic without exposing its platform types.
    pub fn backend(operation: &'static str, error: impl fmt::Display) -> Self {
        Self::Backend {
            operation,
            message: format!("{error:#}"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generate(error) => write!(f, "generate execution module: {error}"),
            Self::Backend { operation, message } => write!(f, "{operation}: {message}"),
            Self::Configuration(error) => error.fmt(f),
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

pub type Result<T> = std::result::Result<T, Error>;

impl From<wasm86_machine::ConfigurationError> for Error {
    fn from(error: wasm86_machine::ConfigurationError) -> Self {
        Self::Configuration(error)
    }
}
