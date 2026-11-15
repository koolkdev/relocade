//! Compilation jobs dispatched through caller-owned worker resources.

use super::CompileExecutor;
use relocade_workers::{JobExecutor, PoolError};
use std::future::Future;

impl CompileExecutor for JobExecutor {
    type Error = PoolError;

    fn execute<T: Send + 'static>(
        &self,
        job: impl FnOnce() -> T + Send + 'static,
    ) -> impl Future<Output = Result<T, Self::Error>> {
        self.execute(job)
    }
}
