//! Compiled code for one CPU. `profiling` collects calls and selects hot functions.
//! `compiler_requests` submits batches and handles replies. `install` publishes dispatch entries.

mod calls;
mod compiler_requests;
mod dispatch;
mod hooks;
mod install;
mod installed;
mod profiling;
mod wake;
mod write_code;

use super::GuestRuntime;
use crate::{
    runtime::{hooks::Handler, memory::HostWritePages},
    worker::HostTurns,
    CompiledCode, CompilerClient, FarPointer, Result,
};
pub(super) use dispatch::DispatchTable;
use hooks::Hooks;
pub(in crate::runtime) use install::Installation;
use installed::InstalledModules;
use profiling::Profiling;
use relocade_wasm_runtime::{Memory, WasmBackend};
use std::{ops::Range, rc::Rc, sync::Arc, task::Context};
pub(crate) use wake::CodeWake;
use wake::CodeWork;
pub(super) use write_code::WriteCode;

pub(in crate::runtime) struct CodeCache {
    // Disarm the page-route owner before dropping its remaining module holds.
    host_writes: Rc<HostWritePages>,
    // The writeCode callback holds these modules while Wasm can run.
    stopped: Option<Box<InstalledModules>>,
    write_code: WriteCode,
    wake: Arc<CodeWake>,
    hooks: Hooks,
    profiling: Profiling,
    compiler: Option<CompilerClient>,
    // Entry points in the one outstanding compiler request.
    pending: Vec<FarPointer>,
    host_turns: HostTurns,
    guest: Rc<GuestRuntime>,
}

impl CodeCache {
    pub(super) fn new(
        guest: Rc<GuestRuntime>,
        dispatch: DispatchTable,
        write_code: WriteCode,
        wake: Arc<CodeWake>,
        call_profile: Memory,
    ) -> Self {
        Self {
            host_writes: HostWritePages::new(&guest.runtime, &guest.bindings),
            stopped: Some(Box::new(InstalledModules::new(dispatch))),
            write_code,
            wake,
            hooks: Hooks::default(),
            profiling: Profiling::new(call_profile),
            compiler: None,
            pending: Vec::new(),
            host_turns: HostTurns::default(),
            guest,
        }
    }

    /// Apply invalidations before publishing replies so a recompiled entry can use its slot.
    pub(in crate::runtime) fn service(&mut self, ready: CodeWork, cx: &mut Context<'_>) {
        if ready.retirements() {
            self.retire_queued().expect("retire modified code");
        }
        if ready.compiler_reply() {
            self.poll_compiler_replies(cx);
        }
    }

    /// A host can patch code between guest entries or while a compiler reply is in flight.
    pub(super) fn retire_pending(&mut self) -> Result<()> {
        if self.wake.take_retirements() {
            self.retire_queued()?;
        }
        Ok(())
    }

    fn retire_queued(&mut self) -> Result<()> {
        let modified = self.wake.drain_modified_modules();
        let installed = self.stopped.as_deref_mut().expect("stopped code cache");
        self.guest
            .runtime
            .with_runtime(|wasm| installed.retire(&mut wasm.context(), modified))
    }

    pub(in crate::runtime) fn invalidate_code(&mut self, range: Range<u64>) -> Result<()> {
        let installed = self.stopped.as_deref_mut().expect("stopped code cache");
        self.guest
            .runtime
            .with_runtime(|wasm| installed.invalidate(&mut wasm.context(), range))
    }

    pub(in crate::runtime) fn compiled_modules_at(
        &self,
        address: u32,
    ) -> impl Iterator<Item = &CompiledCode> {
        self.installed()
            .covering(address)
            .filter_map(|module| module.code())
    }

    fn installed(&self) -> &InstalledModules {
        self.stopped.as_deref().expect("stopped code cache")
    }

    #[must_use]
    pub(super) fn bind_write_callback(&mut self) -> impl Drop + '_ {
        self.write_code.bind(&mut self.stopped)
    }

    pub(in crate::runtime) fn handler(&self, id: u32) -> &Rc<dyn Handler> {
        self.hooks.handler(id)
    }
}
