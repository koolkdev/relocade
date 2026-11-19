//! Collects Real16 call observations and submits hot batches on this CPU's client.
//! Keys are packed 16:16 CS:IP values. Protected-mode dispatch does not record.

use super::{modules::Modules, CpuInstance, WasmRuntime};
use crate::{
    CompiledCode, CompilerClient, Cpu, DiscoveryLimits, FarPointer, FunctionRequest, Instant,
    Result,
};
use futures_channel::mpsc::TryRecvError;
use relocade_wasm_runtime::{Memory, WasmBackend, WasmMemoryAccess};
use relocade_x86::{abi, CallProfile, ExecutionProfile, ExecutionTarget};
use std::time::Duration;
mod hotness;
use hotness::Hotness;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(50);

pub(in crate::runtime) struct Profiling {
    pub(super) memory: Memory,
    hotness: Hotness,
    next_collection: Option<Instant>,
    target: ExecutionTarget,
    compiler: Option<CompilerClient>,
    pending: Vec<FarPointer>,
}

impl Profiling {
    pub(super) fn new(wasm: &mut WasmRuntime, target: ExecutionTarget) -> Result<Self> {
        Ok(Self {
            memory: wasm.create_memory(abi::CALL_PROFILE)?,
            hotness: Hotness::default(),
            next_collection: None,
            target,
            compiler: None,
            pending: Vec::new(),
        })
    }

    /// Collect opportunistically after the interval, without timers or yields.
    fn collect_if_due(
        &mut self,
        wasm: &mut WasmRuntime,
        modules: &Modules<WasmRuntime>,
        now: Instant,
    ) {
        let deadline = *self.next_collection.get_or_insert(now + SAMPLE_INTERVAL);
        if now < deadline {
            return;
        }
        self.collect(wasm, modules);
        self.next_collection = Some(now + SAMPLE_INTERVAL);
    }

    pub(super) fn collect(&mut self, wasm: &mut WasmRuntime, modules: &Modules<WasmRuntime>) {
        {
            let mut memory = wasm.memory(&self.memory);
            let batch = CallProfile::read(|offset| memory.load(offset));
            for entry in batch.iter() {
                self.hotness.record(entry);
            }
            batch.clear(|offset, value| memory.store(offset, value));
        }
        self.submit(modules);
    }

    fn disable(&mut self) {
        self.compiler = None;
        self.pending.clear();
    }

    fn submit(&mut self, modules: &Modules<WasmRuntime>) {
        if self.target.profile != ExecutionProfile::Real16 || !self.pending.is_empty() {
            return;
        }
        let Some(compiler) = &self.compiler else {
            return;
        };
        let entries: Vec<_> = self
            .hotness
            .take_batch(|key| modules.has_entry(key))
            .into_iter()
            .map(FarPointer::from_packed)
            .collect();
        if entries.is_empty() {
            return;
        }
        let requests = entries
            .iter()
            .map(|&entry| FunctionRequest {
                entry,
                limits: DiscoveryLimits::default(),
            })
            .collect();
        if compiler.request_functions(self.target, requests).is_err() {
            self.disable();
        } else {
            self.pending = entries;
        }
    }
}

impl Cpu {
    pub(crate) fn with_compiler(mut self, compiler: Option<CompilerClient>) -> Self {
        self.instance.profiling.compiler = compiler;
        self
    }

    pub(crate) fn take_compiler(&mut self) -> Option<CompilerClient> {
        let profiling = &mut self.instance.profiling;
        assert!(
            profiling.pending.is_empty(),
            "take the compiler before automatic requests begin"
        );
        profiling.compiler.take()
    }
}

impl CpuInstance {
    /// Sample calls and accept a completed batch once per CPU dispatch turn.
    pub(in crate::runtime) fn update_compilation(&mut self) {
        if self.profiling.compiler.is_none()
            || self.profiling.target.profile != ExecutionProfile::Real16
        {
            return;
        }
        self.runtime.with_runtime(|wasm| {
            self.profiling
                .collect_if_due(wasm, &self.modules, Instant::now());
        });
        if self.profiling.pending.is_empty() {
            return;
        }
        let Some(compiler) = &mut self.profiling.compiler else {
            return;
        };
        let reply = match compiler.try_recv() {
            Ok(reply) => reply,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Closed) => {
                self.profiling.disable();
                return;
            }
        };
        let result = reply.and_then(|code| self.install_missing(&code));
        let pending = std::mem::take(&mut self.profiling.pending);
        if matches!(result, Err(crate::Error::StaleCompilation)) {
            // These roots may be hot again once the guest finishes writing.
            self.profiling.hotness.resample(&pending);
        } else {
            for entry in pending {
                if self.install_interpreter_fallback(entry).is_err() {
                    self.profiling.disable();
                    return;
                }
            }
        }
        self.profiling.submit(&self.modules);
    }

    fn install_missing(&mut self, code: &CompiledCode) -> Result<()> {
        let mut entries =
            code.dispatch_entries(self.bindings.cpu.execution_target(), &self.bindings.guest)?;
        entries.retain(|entry| !self.modules.has_entry(entry.key));
        if entries.is_empty() {
            return Ok(());
        }
        self.install_guest_code(code, &entries)
    }
}

#[cfg(test)]
mod tests;
