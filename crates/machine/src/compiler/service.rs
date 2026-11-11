use super::connection::Start;
use crate::{
    worker::{self, Pool, PoolHandle, Receiver, ReceiverEndpoint, Sender, Thread, Worker},
    Result,
};
use futures_util::{
    future::{join, select, Either},
    stream::FuturesUnordered,
    StreamExt,
};
use relocade_wasm_runtime::{WasmBackend, WasmRuntime};
use std::{cell::RefCell, num::NonZeroUsize, pin::pin};

/// Owns the compilation worker and its pool, shared by any number of machines.
/// Work progresses independently after startup; no pool driver needs polling.
/// Shut down the machines, then await [`Self::shutdown`] to join every worker.
pub struct CompilerService {
    inputs: RefCell<Sender<Start>>,
    worker: Worker<Result<()>>,
}

impl CompilerService {
    /// Starts one pool with half the available logical parallelism, rounded down,
    /// and at least one worker. Wasm engine parallelism is separate.
    pub fn start() -> Result<Self> {
        let parallelism = worker::available_parallelism().map_or(1, usize::from);
        let workers = NonZeroUsize::new((parallelism / 2).max(1)).unwrap();
        Self::start_with_workers(workers)
    }

    /// Starts the service with an explicit pool size.
    pub fn start_with_workers(workers: NonZeroUsize) -> Result<Self> {
        let compiler = WasmRuntime::new()?.compiler();
        let (inputs, incoming) = worker::channel_endpoints()?;
        let worker = Worker::spawn_with(incoming, move |incoming, _stopped| {
            run(incoming, compiler, workers)
        });
        Ok(Self {
            inputs: RefCell::new(inputs),
            worker,
        })
    }

    /// Closes CPU admission and joins the service and pool after draining.
    /// All machines must shut down first.
    pub async fn shutdown(self) -> Result<()> {
        drop(self.inputs);
        self.worker.join().await?.transpose()?;
        Ok(())
    }

    pub(super) fn submit(&self, start: Start) -> Result<()> {
        self.inputs.borrow_mut().send(start)?;
        Ok(())
    }
}

async fn run(
    incoming: ReceiverEndpoint<Start>,
    compiler: relocade_wasm_runtime::Compiler,
    workers: NonZeroUsize,
) -> Result<()> {
    // Browser thread owners and the pool driver stay together on this worker.
    let threads = (0..workers.get()).map(|_| Thread::new()).collect();
    let (pool, jobs) = Pool::new(threads)?;
    let ((), result) = join(pool.run(), serve(incoming.bind(), compiler, jobs)).await;
    result
}

async fn serve(
    mut incoming: Receiver<Start>,
    compiler: relocade_wasm_runtime::Compiler,
    jobs: PoolHandle,
) -> Result<()> {
    let mut connections = FuturesUnordered::new();
    loop {
        let next = if connections.is_empty() {
            incoming.recv().await
        } else {
            match select(pin!(incoming.recv()), connections.next()).await {
                Either::Left((next, _)) => next,
                Either::Right(_) => continue,
            }
        };
        let Some(start) = next else { break };
        connections.push(start?.serve(&compiler, &jobs));
    }
    while connections.next().await.is_some() {}
    Ok(())
}
