//! Snapshot generation and backend compilation on an independent host worker.

use super::{Error, Result};
use futures_channel::oneshot;
use relocade_wasm_runtime::{worker::Thread, Compiler, Module, WasmCompiler};
use relocade_x86::{BlockEntry, BlockSnapshot, ExecutionTarget};

/// Owned instruction bytes for one CS-relative entry.
/// The caller maintains snapshot validity, as with x86's `BlockSnapshot`.
pub struct CodeSnapshot {
    pub start_eip: u32,
    pub bytes: Vec<u8>,
    pub instruction_limit: u32,
}

/// Backend code and the instruction ranges used by its exported entries.
/// Instantiation and guest-address registration belong to the execution owner.
pub struct CompiledCode {
    pub(super) module: Module,
    pub(super) entries: Vec<BlockEntry>,
    pub(super) execution_target: ExecutionTarget,
    pub(super) snapshots: Vec<CodeSnapshot>,
}

impl CompiledCode {
    pub fn module(&self) -> &Module {
        &self.module
    }
    pub fn entries(&self) -> &[BlockEntry] {
        &self.entries
    }
    pub fn execution_target(&self) -> ExecutionTarget {
        self.execution_target
    }
}

/// Compiles owned snapshots without accessing the live machine.
///
/// Keep this worker alive and await accepted compilations through completion.
/// Dropping the worker early can terminate its browser thread before Rust
/// destructors run. This primitive does not queue, deduplicate, or validate
/// requests against live guest memory.
pub struct CompilerWorker {
    thread: Thread,
    compiler: Compiler,
    target: ExecutionTarget,
}

impl CompilerWorker {
    pub(super) fn new(compiler: Compiler, target: ExecutionTarget) -> Self {
        Self {
            thread: Thread::new(),
            compiler,
            target,
        }
    }

    pub async fn compile_blocks(&self, snapshots: Vec<CodeSnapshot>) -> Result<CompiledCode> {
        let compiler = self.compiler.clone();
        let target = self.target;
        let (metadata, receive_metadata) = oneshot::channel();
        let task = self.thread.run((), move |()| async move {
            let compiled: Result<_> = async {
                let snapshots: Vec<_> = snapshots
                    .iter()
                    .map(|snapshot| BlockSnapshot {
                        start_eip: snapshot.start_eip,
                        bytes: &snapshot.bytes,
                        instruction_limit: snapshot.instruction_limit,
                    })
                    .collect();
                let blocks = relocade_x86::Compiler::new(target.profile)
                    .with_cpu_model(target.cpu_model)
                    .compile_blocks(&snapshots)?;
                let module = compiler.compile(&blocks.bytes).await?;
                Ok((module, blocks.entries))
            }
            .await;
            // Browser modules cross the JS boundary through structured cloning.
            // Ordinary Rust metadata and typed errors need no serialization.
            match compiled {
                Ok((module, entries)) => {
                    let _ = metadata.send(Ok((entries, snapshots)));
                    Ok(module)
                }
                Err(error) => {
                    let _ = metadata.send(Err(error));
                    Err(())
                }
            }
        });
        // Even compilation errors wait for the worker task to finish first.
        let module = task
            .await
            .map_err(|error| Error::Worker(error.to_string()))?;
        let (entries, snapshots) = receive_metadata
            .await
            .map_err(|error| Error::Worker(error.to_string()))??;
        Ok(CompiledCode {
            module: module.expect("successful metadata accompanies a compiled module"),
            entries,
            execution_target: target,
            snapshots,
        })
    }
}
