//! Caller-provided execution of independent compilation jobs.

use crate::BuildError;
use std::{error::Error, fmt, future::Future};

/// Runs owned compilation jobs using the caller's execution resources.
///
/// A job and its result can move between threads. The executor and its returned
/// future need not be `Send`: browser worker handles stay on their owner.
/// Implementations control admission, concurrency and worker lifetime. They must
/// execute each accepted job at most once and report failures to its waiter.
/// Dropping a waiter may leave accepted work running; every job owns its inputs.
///
/// Compilation submits multiple jobs before awaiting their completion. An
/// executor may run jobs immediately or admit them when their futures are polled.
pub trait CompileExecutor {
    type Error;

    fn execute<T: Send + 'static>(
        &self,
        job: impl FnOnce() -> T + Send + 'static,
    ) -> impl Future<Output = Result<T, Self::Error>>;
}

/// Distinguishes invalid function construction from failure to execute a job.
#[derive(Debug)]
pub enum CompileError<E> {
    Build(BuildError),
    Execution(E),
}

impl<E> From<BuildError> for CompileError<E> {
    fn from(error: BuildError) -> Self {
        Self::Build(error)
    }
}

impl<E: fmt::Display> fmt::Display for CompileError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Build(error) => error.fmt(formatter),
            Self::Execution(error) => write!(formatter, "execute compilation job: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for CompileError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(match self {
            Self::Build(error) => error,
            Self::Execution(error) => error,
        })
    }
}
