//! Sample call counts and compile hot Real16 functions for this CPU.
//! Keys are packed 16:16 CS:IP values. Protected-mode dispatch does not record.

use super::{CodeCache, Installation, WasmRuntime};
use crate::{
    CompiledCode, CompilerClient, Cpu, DiscoveryLimits, FarPointer, FunctionRequest, Instant,
    Result,
};
use relocade_wasm_runtime::{Memory, WasmBackend, WasmMemoryAccess};
use relocade_x86::{abi, CallProfile, ExecutionProfile};
use std::{
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
}

impl Profiling {
    pub(super) fn new(wasm: &mut WasmRuntime) -> Result<Self> {
        Ok(Self {
            memory: wasm.create_memory(abi::CALL_PROFILE)?,
            hotness: Hotness::default(),
            next_collection: None,
        })
    }

    /// Collect opportunistically after the interval, without timers or yields.
    fn collection_due(&mut self, now: Instant) -> bool {
        let deadline = *self.next_collection.get_or_insert(now + SAMPLE_INTERVAL);
        if now < deadline {
            return false;
        }
        self.next_collection = Some(now + SAMPLE_INTERVAL);
        true
    }

    fn collect(&mut self, wasm: &mut WasmRuntime) {
        let mut memory = wasm.memory(&self.memory);
        let batch = CallProfile::read(|offset| memory.load(offset));
        for entry in batch.iter() {
            self.hotness.record(entry);
        }
        batch.clear(|offset, value| memory.store(offset, value));
    }
}

impl CodeCache {
    fn disable_compiler(&mut self) {
        self.compiler = None;
        self.pending.clear();
    }

    fn submit_hot_functions(&mut self) {
        let target = self.guest.bindings.cpu.execution_target();
        if target.profile != ExecutionProfile::Real16 || !self.pending.is_empty() {
            return;
        }
        let Some(compiler) = &self.compiler else {
            return;
        };
        let entries: Vec<_> = self
            .profiling
            .hotness
            .take_batch(|key| self.modules.has_entry(key))
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
        if compiler.request_functions(target, requests).is_err() {
            self.disable_compiler();
        } else {
            self.pending = entries;
        }
    }

    pub(super) fn collect_calls(&mut self) {
        self.guest
            .runtime
            .with_runtime(|wasm| self.profiling.collect(wasm));
        self.submit_hot_functions();
    }

    pub(in crate::runtime) fn sample_calls(&mut self) {
        if self.compiler.is_some()
            && self.guest.bindings.cpu.execution_target().profile == ExecutionProfile::Real16
            && self.profiling.collection_due(Instant::now())
        {
            self.collect_calls();
        }
    }

    /// Drain the local slot and register the code cache for the next delivery.
    pub(super) fn poll_compiler_replies(&mut self, cx: &mut Context<'_>) {
        while let Some(compiler) = &mut self.compiler {
            match compiler.poll_reply(cx) {
                Poll::Pending => return,
                Poll::Ready(None) => self.disable_compiler(),
                Poll::Ready(Some(reply)) => self.install_compiler_reply(reply),
            }
        }
    }

    fn install_compiler_reply(&mut self, reply: Result<CompiledCode>) {
        let pending = std::mem::take(&mut self.pending);
        // Bound calls bypass the recorder. Retirement makes fresh calls eligible again.
        self.profiling.hotness.reset_counts(&pending);
        let result = reply.and_then(|code| {
            code.check_destination(
                self.guest.bindings.cpu.execution_target(),
                &self.guest.bindings.guest,
            )?;
            self.install_guest_code(&code, Installation::Profiled(&pending))
        });
        match result {
            Ok(()) | Err(crate::Error::StaleCompilation) => {}
            Err(_) => {
                if self.bind_interpreter_if_vacant(pending).is_err() {
                    self.disable_compiler();
                    return;
                }
            }
        }
        self.submit_hot_functions();
    }
}

impl Cpu {
    pub(crate) fn with_compiler(mut self, compiler: Option<CompilerClient>) -> Self {
        if compiler.is_some() {
            self.memory().code_wake().compiler_connected();
        }
        self.instance.code.compiler = compiler;
        self
    }

    pub(crate) fn take_compiler(&mut self) -> Option<CompilerClient> {
        let code = &mut self.instance.code;
        assert!(
            code.pending.is_empty(),
            "take the compiler before automatic requests begin"
        );
        code.compiler.take()
    }
}

#[cfg(test)]
mod tests;
