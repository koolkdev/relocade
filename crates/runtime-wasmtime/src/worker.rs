use futures_channel::oneshot;
use relocade_runtime::{
    compilation::{BackgroundCompiler, Compilation},
    CodeSnapshot, CompiledGroup, Error, Result,
};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use wasmtime::{Engine, Module};

struct Job {
    snapshot: CodeSnapshot,
    result: oneshot::Sender<Result<Option<CompiledGroup<Module>>>>,
}

pub(super) struct CompilerWorker {
    jobs: SyncSender<Job>,
}

impl CompilerWorker {
    pub fn new(engine: Engine) -> Result<Self> {
        let (jobs, receiver) = mpsc::sync_channel::<Job>(1);
        std::thread::Builder::new()
            .name("relocade-compiler".into())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    if job.result.is_canceled() {
                        continue;
                    }
                    let result = job.snapshot.compile().and_then(|prepared| {
                        prepared
                            .map(|prepared| {
                                Module::new(&engine, prepared.wasm())
                                    .map(|module| CompiledGroup::new(prepared, module))
                                    .map_err(|error| Error::wasm("compile background Wasm", error))
                            })
                            .transpose()
                    });
                    let _ = job.result.send(result);
                }
            })
            .map_err(|error| Error::wasm("start compiler thread", error))?;
        // Closing the sender lets the worker finish accepted work and exit.
        // Dropping a runtime must never join a thread that is still compiling.
        Ok(Self { jobs })
    }
}

impl BackgroundCompiler<Module> for CompilerWorker {
    fn submit(&mut self, snapshot: CodeSnapshot) -> Result<Option<Compilation<Module>>> {
        let (result, receiver) = oneshot::channel();
        match self.jobs.try_send(Job { snapshot, result }) {
            Ok(()) => Ok(Some(Box::pin(async move {
                receiver
                    .await
                    .map_err(|error| Error::wasm("receive compiler result", error))?
            }))),
            Err(TrySendError::Full(_)) => Ok(None),
            Err(TrySendError::Disconnected(_)) => {
                Err(Error::wasm("submit compilation", "compiler thread stopped"))
            }
        }
    }
}
