//! Collects Real16 call observations and submits hot batches on this CPU's client.
//! Keys are packed 16:16 CS:IP values. Protected-mode dispatch does not record.

use super::{table::DispatchTable, WasmRuntime};
use crate::{CompilerClient, Cpu, FarPointer, FunctionRequest, Instant, Result};
use relocade_wasm_runtime::{Memory, WasmBackend, WasmMemoryAccess};
use relocade_x86::{abi, CallProfile, ExecutionProfile, ExecutionTarget, FunctionLimits};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll},
    time::Duration,
};
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
        table: &DispatchTable<WasmRuntime>,
        now: Instant,
    ) {
        let deadline = *self.next_collection.get_or_insert(now + SAMPLE_INTERVAL);
        if now < deadline {
            return;
        }
        self.collect(wasm, table);
        self.next_collection = Some(now + SAMPLE_INTERVAL);
    }

    pub(super) fn collect(&mut self, wasm: &mut WasmRuntime, table: &DispatchTable<WasmRuntime>) {
        {
            let mut memory = wasm.memory(&self.memory);
            let batch = CallProfile::read(|offset| memory.load(offset));
            for entry in batch.iter() {
                self.hotness.record(entry);
            }
            batch.clear(|offset, value| memory.store(offset, value));
        }
        self.submit(table);
    }

    fn disable(&mut self) {
        self.compiler = None;
        self.pending.clear();
    }

    fn submit(&mut self, table: &DispatchTable<WasmRuntime>) {
        if self.target.profile != ExecutionProfile::Real16 || !self.pending.is_empty() {
            return;
        }
        let Some(compiler) = &self.compiler else {
            return;
        };
        let entries: Vec<_> = self
            .hotness
            .take_batch(|key| table.contains(FarPointer::from_packed(key).address()))
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
                limits: FunctionLimits::default(),
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

    pub(in crate::runtime) fn poll_compilation(&mut self, cx: &mut Context<'_>) {
        let instance = &mut self.instance;
        if instance.profiling.compiler.is_none()
            || instance.profiling.target.profile != ExecutionProfile::Real16
        {
            return;
        }
        instance.runtime.with_runtime(|wasm| {
            instance
                .profiling
                .collect_if_due(wasm, &instance.table, Instant::now());
        });
        // Host services may retain dispatch entries. Keep replies queued until
        // installation can use the ordinary CPU API.
        if self.pending.is_some() || !self.suspended.is_empty() {
            return;
        }
        loop {
            let Some(compiler) = &mut self.instance.profiling.compiler else {
                return;
            };
            // Register a receive even before a request exists: a full-buffer
            // collection can submit while guest execution is still running.
            let reply = pin!(compiler.recv()).poll(cx);
            let reply = match reply {
                Poll::Pending => return,
                Poll::Ready(Some(reply)) => reply,
                Poll::Ready(None) => {
                    self.instance.profiling.disable();
                    return;
                }
            };
            if let Ok(code) = reply {
                // Missing roots fall back below if module installation fails.
                let _ = code.install_missing(self);
            }
            for entry in std::mem::take(&mut self.instance.profiling.pending) {
                if self.instance.interpret_at(entry.address()).is_err() {
                    self.instance.profiling.disable();
                    return;
                }
            }
            // Observations may have accumulated while the previous batch ran.
            let instance = &mut self.instance;
            instance.profiling.submit(&instance.table);
        }
    }
}

#[cfg(test)]
mod tests;
