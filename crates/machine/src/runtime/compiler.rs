//! One persistent compiler over a guest's shared memory and fixed execution target.

use super::{Error, Result};
use crate::Memory;
use futures_channel::{mpsc, oneshot};
use futures_util::StreamExt;
use relocade_wasm_runtime::{
    worker::{self, Port, SendTask, Thread},
    Backend, Module, WasmBackend, WasmCompiler,
};
use relocade_x86::{BlockEntry, BlockSnapshot, ExecutionTarget};
use std::future::poll_fn;

/// A bounded instruction range to read from the compiler's guest memory.
/// `address` is the guest physical address before A20 and physical mapping;
/// `start_eip` is the CS-relative entry exported by the generated block.
#[derive(Clone, Copy, Debug)]
pub struct BlockRequest {
    pub address: u32,
    pub start_eip: u32,
    pub byte_len: usize,
    pub instruction_limit: u32,
}

/// Prepared code and the instruction ranges used by its exported entries.
/// Each execution owner binds the module to its own CPU, memories and dispatch.
pub struct CompiledCode {
    pub module: Module,
    pub entries: Vec<BlockEntry>,
    pub execution_target: ExecutionTarget,
}

struct Request {
    id: u64,
    blocks: Vec<BlockRequest>,
    reply: oneshot::Sender<Result<Vec<BlockEntry>>>,
}

/// A persistent worker scoped to shared guest memory and one execution target.
/// Requests and typed results use Rust channels; compiled modules use resource
/// transport. Guest backing is attached once, without copying bytes per request.
///
/// Keep requested code and mappings unchanged until the result is received.
/// Installation and subsequent code validity remain the caller's responsibility.
/// Await startup and shutdown through completion. Dropping this owner early has
/// Thread's browser limitation: remote Rust destructors may not run.
pub struct CompilerWorker {
    _thread: Thread,
    task: SendTask<Result<()>>,
    requests: mpsc::Sender<Request>,
    modules: Port<(u64, Module)>,
    next_id: u64,
    target: ExecutionTarget,
}

impl CompilerWorker {
    pub async fn start(memory: &Memory, target: ExecutionTarget) -> Result<Self> {
        let compiler = Backend::new()?.compiler();
        let config = memory.configuration().clone();
        let (output, input) = worker::channel()?;
        let (requests, mut incoming) = mpsc::channel::<Request>(8);
        let (ready, started) = oneshot::channel();
        let thread = Thread::new();
        let task = thread.run_send(
            (memory.backing().clone(), output),
            move |(backing, output)| async move {
                let memory = Memory::with_backing(config, backing)?;
                let output = output.bind();
                let _ = ready.send(());
                while let Some(request) = incoming.next().await {
                    if request.reply.is_canceled() {
                        continue;
                    }
                    let result = async {
                        let captured = capture(&memory, &request.blocks)?;
                        let snapshots: Vec<_> = request
                            .blocks
                            .iter()
                            .zip(&captured)
                            .map(|(block, bytes)| BlockSnapshot {
                                start_eip: block.start_eip,
                                bytes,
                                instruction_limit: block.instruction_limit,
                            })
                            .collect();
                        let blocks = relocade_x86::Compiler::new(target.profile)
                            .with_cpu_model(target.cpu_model)
                            .compile_blocks(&snapshots)?;
                        let module = compiler.compile(&blocks.bytes).await?;
                        Ok::<_, Error>((module, blocks.entries))
                    }
                    .await;
                    match result {
                        Ok((module, entries)) => {
                            if !request.reply.is_canceled() {
                                output.send((request.id, module))?;
                                let _ = request.reply.send(Ok(entries));
                            }
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                Ok(())
            },
        );
        if started.await.is_err() {
            task.await.map_err(worker_error)??;
            return Err(Error::Worker("compiler stopped during startup".into()));
        }
        Ok(Self {
            _thread: thread,
            task,
            requests,
            modules: input.bind(),
            next_id: 0,
            target,
        })
    }

    /// Compiles mapped guest ranges on the existing worker. Dropping the waiter
    /// does not stop that worker; a later request discards any abandoned output.
    pub async fn compile_blocks(&mut self, blocks: Vec<BlockRequest>) -> Result<CompiledCode> {
        poll_fn(|cx| self.requests.poll_ready(cx))
            .await
            .map_err(worker_error)?;
        let id = self.next_id;
        self.next_id = id.checked_add(1).expect("compiler request ID exhausted");
        let (reply, result) = oneshot::channel();
        self.requests
            .start_send(Request { id, blocks, reply })
            .map_err(worker_error)?;
        let entries = result.await.map_err(worker_error)??;
        loop {
            let (received, module) = self
                .modules
                .recv()
                .await
                .ok_or_else(|| Error::Worker("compiler result channel closed".into()))??;
            if received == id {
                return Ok(CompiledCode {
                    module,
                    entries,
                    execution_target: self.target,
                });
            }
            // The exclusive request waiter can be dropped after its module was
            // posted. IDs keep that late module separate from its successor.
            assert!(received < id, "compiler results preserve request order");
        }
    }

    /// Closes submission, drains accepted requests and releases the worker only
    /// after its persistent loop has dropped its resources on that worker.
    pub async fn shutdown(mut self) -> Result<()> {
        self.requests.close_channel();
        self.task.await.map_err(worker_error)?
    }
}

fn capture(memory: &Memory, blocks: &[BlockRequest]) -> Result<Vec<Vec<u8>>> {
    blocks
        .iter()
        .map(|block| Ok(memory.view().read_ram_rom(block.address, block.byte_len)?))
        .collect()
}

fn worker_error(error: impl std::fmt::Display) -> Error {
    Error::Worker(error.to_string())
}

#[cfg(test)]
mod tests;
