use std::fmt;

/// A PC configuration was rejected or its execution environment failed to initialize.
#[derive(Debug)]
pub enum InitializationError {
    Validation(String),
    Machine(relocade_machine::Error),
}

impl fmt::Display for InitializationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(message) => f.write_str(message),
            Self::Machine(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for InitializationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(_) => None,
            Self::Machine(error) => Some(error),
        }
    }
}

impl From<relocade_machine::Error> for InitializationError {
    fn from(error: relocade_machine::Error) -> Self {
        Self::Machine(error)
    }
}
