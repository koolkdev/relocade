//! Machine ownership of execution, compilation and host timers.

use super::{
    compilation::{Compilation, CompilerInput},
    ExecutionWorker, Runner,
};
use crate::{compiler::worker_error, timer_service, worker, Result};
use futures_channel::oneshot;
use futures_lite::future::or;
use std::future::Future;

/// Runs a machine session with runtime-owned compilation and host timers.
/// Construct the CPU with `Runner::create_cpu`; `Runner::compile_blocks` starts
/// the compiler only on first use. Requests and responses then travel directly
/// between execution and compilation, using the CPU's existing shared memory.
///
/// Poll on the responsive host and await through cleanup. Completion or `stop`
/// closes submission and joins workers after accepted compilation finishes.
/// `None` means the stop signal ended the session. Low-level `ExecutionWorker`
/// remains independently scheduled and does not require this host service.
pub async fn run_session<T: Send + 'static>(
    stop: impl Future,
    session: impl AsyncFnOnce(&Runner) -> T + Send + 'static,
) -> Result<Option<T>> {
    let (output, input) = worker::channel::<CompilerInput>().map_err(worker_error)?;
    let mut input = input.bind();
    let (cancel, stopped) = oneshot::channel::<()>();
    let (timers, driver) = timer_service();
    let execution = ExecutionWorker::spawn_runner(
        move |storage, endpoint, output| {
            let mut runner = Runner::with_host_timers(timers, storage, endpoint);
            runner.compilation = Some(Compilation::new(output));
            runner
        },
        output,
        async move |runner| {
            or(
                async {
                    let _ = stopped.await;
                    None
                },
                async { Some(session(runner).await) },
            )
            .await
        },
    );
    let timers = driver.spawn_with_peer(execution.wake_peer());
    let compilation = async {
        match input.recv().await {
            Some(source) => source.map_err(worker_error)?.run().await,
            None => Ok(()),
        }
    };
    let cancel_session = async {
        stop.await;
        let _ = cancel.send(());
        // Keep compiler startup and accepted work driven through shutdown.
        std::future::pending().await
    };
    // A compiler setup failure also drops the stop sender, ending execution
    // before join even if the callback handled its connection error.
    let compiled = or(cancel_session, compilation).await;
    let result = execution.join().await.map_err(worker_error);
    let timer_result = timers.stop().await.map_err(worker_error);
    let result = result?.flatten();
    timer_result?;
    compiled?;
    Ok(result)
}
