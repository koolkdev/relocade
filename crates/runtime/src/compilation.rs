//! Bounded background work owned by the runtime, independent of a run future.

use crate::{CodeSnapshot, CompiledGroup, Result, Runtime, RuntimeBackend, RuntimeState};
use relocade_x86::HotEntry;
use std::{future::Future, pin::Pin, task::Poll};

/// Completion of accepted background work. Polling only observes the worker;
/// it must never discover blocks, lower code or compile Wasm on the caller.
pub type Compilation<M> = Pin<Box<dyn Future<Output = Result<Option<CompiledGroup<M>>>> + 'static>>;

/// A compiler independent of the live guest and its engine store. Submitting
/// work never waits for queue capacity. Inputs and completions are owned.
pub trait BackgroundCompiler<M> {
    /// None means the bounded worker queue declined this job. An accepted future
    /// may itself resolve to None when the snapshot cannot be translated.
    fn submit(&mut self, snapshot: CodeSnapshot) -> Result<Option<Compilation<M>>>;
}

pub(super) struct CompilationQueue<M> {
    compiler: Option<Box<dyn BackgroundCompiler<M>>>,
    pending: Vec<Pending<M>>,
}

struct Pending<M> {
    entry: HotEntry,
    completion: Compilation<M>,
}

impl<M> CompilationQueue<M> {
    pub fn new(compiler: Option<Box<dyn BackgroundCompiler<M>>>) -> Self {
        Self {
            compiler,
            pending: Vec::new(),
        }
    }

    pub fn can_queue(&self, entry: HotEntry) -> bool {
        self.compiler.is_some()
            && self.pending.len() < 2
            && !self.pending.iter().any(|pending| pending.entry == entry)
    }
}

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Runtime<B> {
    /// Selects a compiler for subsequent requests. Already accepted jobs retain
    /// their slots and completion futures. Host state and live imports never
    /// cross this interface. None leaves future requests in the interpreter.
    pub fn set_compiler(&mut self, compiler: Option<Box<dyn BackgroundCompiler<B::Module>>>) {
        self.compilation.compiler = compiler;
    }

    pub fn has_compiler(&self) -> bool {
        self.compilation.compiler.is_some()
    }

    /// Queues at most two jobs, deduplicated by their seed CS.base/EIP. Returns
    /// false for another runtime's capture, an unavailable compiler or a full
    /// queue. No discovery or engine compilation runs in this call.
    pub fn queue_group(&mut self, snapshot: CodeSnapshot) -> Result<bool> {
        let entry = snapshot.entry();
        if !snapshot.belongs_to(&self.storage.code.as_ref().unwrap().state)
            || !self.compilation.can_queue(entry)
        {
            return Ok(false);
        }
        let Some(completion) = self
            .compilation
            .compiler
            .as_mut()
            .unwrap()
            .submit(snapshot)?
        else {
            return Ok(false);
        };
        self.compilation.pending.push(Pending { entry, completion });
        Ok(true)
    }

    pub fn pending_compilation_count(&self) -> usize {
        self.compilation.pending.len()
    }

    /// Publishes ready results without waiting for unfinished compilation. Engine
    /// instantiation binds private guest imports on this execution owner. Dropping
    /// this future never drops jobs still running in the background.
    pub async fn publish_ready_groups(&mut self) -> Result<usize> {
        let mut published = 0;
        let mut index = 0;
        while index < self.compilation.pending.len() {
            let result = std::future::poll_fn(|cx| {
                Poll::Ready(self.compilation.pending[index].completion.as_mut().poll(cx))
            })
            .await;
            match result {
                Poll::Pending => index += 1,
                Poll::Ready(result) => {
                    drop(self.compilation.pending.remove(index));
                    if let Some(group) = result? {
                        published += usize::from(self.publish_group(group).await?.is_some());
                    }
                }
            }
        }
        Ok(published)
    }
}
