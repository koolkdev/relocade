//! The worker owns generation and engine compilation; stores never cross threads.

use super::CompiledEntry;
use std::{
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
    thread,
};
use wasm86_codegen::Request;
use wasmtime::{Engine, Module};

pub(super) const MAX_PENDING: usize = 8;

pub(super) struct Job {
    pub id: u64,
    pub request: Request,
}
pub(super) struct Completion {
    pub id: u64,
    pub result: Result<CompiledEntry, String>,
}

pub(super) struct Worker {
    sender: SyncSender<Job>,
    pub completions: Receiver<Completion>,
}

pub(super) fn compile(engine: &Engine, request: Request) -> Result<CompiledEntry, String> {
    let generated = request.generate()?;
    let module = Module::new(engine, generated.bytes).map_err(|error| error.to_string())?;
    Ok(CompiledEntry {
        module,
        entry: generated.entry,
    })
}

impl Worker {
    pub fn start(engine: Engine) -> std::io::Result<Self> {
        Self::spawn(move |request| compile(&engine, request))
    }

    pub fn spawn(
        mut compile: impl FnMut(Request) -> Result<CompiledEntry, String> + Send + 'static,
    ) -> std::io::Result<Self> {
        let (sender, requests) = mpsc::sync_channel::<Job>(MAX_PENDING);
        let (completed, completions) = mpsc::channel();
        thread::Builder::new()
            .name("wasm86-compiler".into())
            .spawn(move || {
                while let Ok(job) = requests.recv() {
                    let result = compile(job.request);
                    if completed.send(Completion { id: job.id, result }).is_err() {
                        break;
                    }
                }
            })?;
        // Dropping a runtime disconnects the channels without joining a compiler
        // that may still be working. It exits after its current request finishes.
        Ok(Self {
            sender,
            completions,
        })
    }

    pub fn submit(&self, job: Job) -> Result<(), super::SubmitError> {
        self.sender.try_send(job).map_err(|error| match error {
            TrySendError::Full(_) => super::SubmitError::Full,
            TrySendError::Disconnected(_) => super::SubmitError::Stopped,
        })
    }
}
