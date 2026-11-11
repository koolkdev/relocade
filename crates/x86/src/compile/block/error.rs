use crate::BlockError;
use relocade_compiler::CompileError;
use std::{error::Error, fmt};

/// Distinguishes block construction errors from failure to execute a compiler job.
#[derive(Debug)]
pub enum BlockCompileError<E> {
    Block(BlockError),
    Execution(E),
}

impl<E> From<BlockError> for BlockCompileError<E> {
    fn from(error: BlockError) -> Self {
        Self::Block(error)
    }
}

impl<E> From<CompileError<E>> for BlockCompileError<E> {
    fn from(error: CompileError<E>) -> Self {
        match error {
            CompileError::Build(error) => Self::Block(error.into()),
            CompileError::Execution(error) => Self::Execution(error),
        }
    }
}

impl<E: fmt::Display> fmt::Display for BlockCompileError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Block(error) => error.fmt(formatter),
            Self::Execution(error) => write!(formatter, "execute compilation job: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for BlockCompileError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(match self {
            Self::Block(error) => error,
            Self::Execution(error) => error,
        })
    }
}
