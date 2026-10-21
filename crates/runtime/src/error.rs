use relocade_compiler::BuildError;
use relocade_machine::ConfigurationError;
use std::fmt;

/// Host setup/execution errors, separate from architectural guest exits.
#[derive(Debug)]
pub enum Error {
    Generate(BuildError),
    Configuration(ConfigurationError),
    Wasm(relocade_wasm_runtime::Error),
    InvalidExit(u64),
    CancelledHostCall,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generate(error) => write!(f, "generate execution module: {error}"),
            Self::Configuration(error) => error.fmt(f),
            Self::Wasm(error) => error.fmt(f),
            Self::InvalidExit(word) => write!(f, "invalid guest exit word {word:#018x}"),
            Self::CancelledHostCall => {
                write!(
                    f,
                    "host call cancelled; replace its guest continuation before resuming"
                )
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Generate(error) => Some(error),
            Self::Configuration(error) => Some(error),
            Self::Wasm(error) => Some(error),
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

impl From<relocade_wasm_runtime::Error> for Error {
    fn from(error: relocade_wasm_runtime::Error) -> Self {
        Self::Wasm(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
