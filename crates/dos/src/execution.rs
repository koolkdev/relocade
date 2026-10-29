use crate::{Dos, ProcessStatus, ServiceError};
use relocade_machine::Runner;
use relocade_x86::ExecutionExit;
use std::fmt;

/// A program did not finish with an ordinary DOS exit code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunError {
    NoProcess,
    Service(ServiceError),
    /// Execution stopped without DOS terminating the current process.
    Execution(ExecutionExit),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoProcess => f.write_str("no DOS program has been launched"),
            Self::Service(error) => error.fmt(f),
            Self::Execution(exit) => write!(f, "guest execution stopped: {exit:?}"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Service(error) => Some(error),
            _ => None,
        }
    }
}

impl Dos {
    /// Runs the current program and interprets its DOS termination status.
    /// A completed process is not executed again; another launch reuses the same
    /// environment and video output. Unexpected CPU exits leave the process active
    /// for inspection.
    ///
    /// Process completion leaves the environment alive. Keep the main session
    /// alive to capture the program's final writes and advance idle video.
    /// Session shutdown uses the execution worker's separate `stop().await`.
    pub async fn run(&mut self, runner: &Runner) -> Result<u8, RunError> {
        let process = self.process().ok_or(RunError::NoProcess)?;
        if let ProcessStatus::Exited(result) = process.status() {
            return result.map_err(RunError::Service);
        }
        let exit = runner.run_cpu(&mut self.cpu).await;
        match process.status() {
            ProcessStatus::Exited(result) => result.map_err(RunError::Service),
            ProcessStatus::Active => Err(RunError::Execution(exit)),
        }
    }
}
