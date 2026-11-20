//! Submits batches to the compiler and handles replies for this CPU.

use super::{install::Installation, CodeCache};
use crate::{CompiledCode, CompilerClient, Cpu, DiscoveryLimits, FunctionRequest, Result};
use relocade_x86::ExecutionMode;
use std::task::{Context, Poll};

impl CodeCache {
    pub(super) fn submit_hot_functions(&mut self) {
        let config = self.guest.bindings.cpu.compiler_config();
        if config.entry.mode != ExecutionMode::Real || !self.pending.is_empty() {
            return;
        }
        let Some(compiler) = &self.compiler else {
            return;
        };
        let installed = self.stopped.as_deref().expect("stopped code cache");
        let entries = self.profiling.take_hot_functions(&installed.dispatch);
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
        if compiler.request_functions(config, requests).is_err() {
            self.disable_compiler();
        } else {
            self.pending = entries;
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
        self.profiling.reset_counts(&pending);
        let result = reply.and_then(|code| {
            code.check_destination(
                self.guest.bindings.cpu.compiler_config(),
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

    fn disable_compiler(&mut self) {
        self.compiler = None;
        self.pending.clear();
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
