//! Compiled code and automatic compilation for one CPU.

mod calls;
mod dispatch;
mod hooks;
mod installed;
mod profiling;
mod wake;
mod write_code;

use super::{DispatchEntry, GuestRuntime};
use crate::{
    runtime::{hooks::Handler, memory::HostWritePages},
    worker::HostTurns,
    CompilationBinding, CompiledCode, CompilerClient, Cpu, DiscoveryLimits, FarPointer,
    FunctionRequest, Hook, Instant, Result,
};
use calls::Calls;
pub(super) use dispatch::DispatchTable;
use hooks::{Hooks, PreparedHandler};
use installed::{InstalledModules, ModuleInstance};
use profiling::Profiling;
use relocade_wasm_runtime::{Memory, Module, WasmBackend, WasmCompiler};
use relocade_x86::{abi, ExecutionProfile};
use std::{
    ops::Range,
    rc::Rc,
    task::{Context, Poll},
};
pub(crate) use wake::CodeWake;
use wake::CodeWork;
pub(super) use write_code::WriteCode;

#[derive(Clone, Copy)]
pub(in crate::runtime) enum Installation<'a> {
    Explicit,
    Profiled(&'a [FarPointer]),
}

enum ModuleSource<'a> {
    Wasm(&'a Module),
    Guest(&'a CompiledCode),
}

pub(in crate::runtime) struct CodeCache {
    // Disarm the page-route owner before dropping its remaining module holds.
    host_writes: Rc<HostWritePages>,
    stopped: Option<Box<InstalledModules>>,
    write_code: WriteCode,
    hooks: Hooks,
    profiling: Profiling,
    compiler: Option<CompilerClient>,
    pending: Vec<FarPointer>,
    host_turns: HostTurns,
    guest: Rc<GuestRuntime>,
}

impl CodeCache {
    pub(super) fn new(
        guest: Rc<GuestRuntime>,
        dispatch: DispatchTable,
        write_code: WriteCode,
        call_profile: Memory,
    ) -> Self {
        Self {
            host_writes: HostWritePages::new(&guest.runtime, &guest.bindings),
            stopped: Some(Box::new(InstalledModules::new(dispatch))),
            write_code,
            hooks: Hooks::default(),
            profiling: Profiling::new(call_profile),
            compiler: None,
            pending: Vec::new(),
            host_turns: HostTurns::default(),
            guest,
        }
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
        if self.guest.bindings.guest.code_wake().take_retirements() {
            self.retire_queued()?;
        }
        Ok(())
    }

    fn retire_queued(&mut self) -> Result<()> {
        let modified = self.guest.bindings.guest.drain_modified_modules();
        let installed = self.stopped.as_deref_mut().expect("stopped code cache");
        self.guest
            .runtime
            .with_runtime(|wasm| installed.retire(&mut wasm.context(), modified))
    }

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
        let installed = self.stopped.as_deref().expect("stopped code cache");
        let entries: Vec<_> = self
            .profiling
            .hotness
            .take_batch(|key| installed.dispatch.contains(key))
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
    fn poll_compiler_replies(&mut self, cx: &mut Context<'_>) {
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

    async fn compile_wasm(&self, bytes: &[u8]) -> Result<Module> {
        let compiler = self.guest.runtime.with_runtime(|wasm| wasm.compiler());
        Ok(self.host_turns.wait(compiler.compile(bytes)).await?)
    }

    pub(in crate::runtime) async fn install(
        &mut self,
        hooks: impl IntoIterator<Item = Hook>,
    ) -> Result<()> {
        let mut hooks = self.hooks.compile(hooks)?;
        let module = self.compile_wasm(&hooks.bytes).await?;
        let handlers = hooks.take_handlers();
        self.install_module(ModuleSource::Wasm(&module), &hooks.entries(), handlers)
    }

    pub(in crate::runtime) async fn install_wasm_bytes(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let module = self.compile_wasm(bytes).await?;
        self.install_wasm_module(&module, entries)
    }

    pub(in crate::runtime) fn install_wasm_module(
        &mut self,
        module: &Module,
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        self.install_module(ModuleSource::Wasm(module), entries, Vec::new())
    }

    pub(in crate::runtime) fn install_guest_code(
        &mut self,
        code: &CompiledCode,
        installation: Installation<'_>,
    ) -> Result<()> {
        self.retire_pending()?;
        let mut exports = Vec::new();
        let mut fallbacks = Vec::new();
        for binding in code.bindings() {
            match binding {
                CompilationBinding::Export { entry, export } => {
                    let key = entry.packed();
                    if matches!(installation, Installation::Explicit)
                        || !self.installed().dispatch.contains(key)
                    {
                        exports.push(DispatchEntry { key, export });
                    }
                }
                CompilationBinding::Interpreter(entry) => fallbacks.push(entry),
            }
        }
        // A stale module must fail before any of this reply's entries are bound.
        if !exports.is_empty() {
            self.install_module(ModuleSource::Guest(code), &exports, Vec::new())?;
        }
        // A partially decoded profiling batch can also leave a requested root unbound.
        let remaining_roots = match installation {
            Installation::Explicit => &[][..],
            Installation::Profiled(roots) => roots,
        };
        self.bind_interpreter_if_vacant(
            fallbacks.into_iter().chain(remaining_roots.iter().copied()),
        )
    }

    pub(super) fn bind_interpreter_if_vacant(
        &mut self,
        entries: impl IntoIterator<Item = FarPointer>,
    ) -> Result<()> {
        let installed = self.stopped.as_deref_mut().expect("stopped code cache");
        self.guest
            .runtime
            .with_runtime(|wasm| installed.dispatch.bind_interpreter_if_vacant(wasm, entries))
    }

    fn install_module(
        &mut self,
        source: ModuleSource<'_>,
        entries: &[DispatchEntry<'_>],
        handlers: Vec<PreparedHandler>,
    ) -> Result<()> {
        let guest = self.guest.bindings.guest.clone();
        let checked_source = match source {
            ModuleSource::Wasm(_) => None,
            ModuleSource::Guest(code) => {
                let source = code.source();
                Some((source, source.lock_current_bytes(&guest)?))
            }
        };
        let instance = self.instantiate_module(source)?;
        let modules = self.stopped.as_deref_mut().expect("stopped code cache");
        let (id, installed) = self
            .guest
            .runtime
            .with_runtime(|wasm| modules.register_exports(wasm, instance, entries))?;
        if let Some((source, mut registry)) = checked_source {
            installed.track_source(registry.register(id, source), &self.host_writes);
        }
        // Releasing a host callback can reenter the runtime or write guest memory.
        self.hooks.replace_bindings(entries, handlers);
        Ok(())
    }

    fn instantiate_module(&mut self, source: ModuleSource<'_>) -> Result<ModuleInstance> {
        let guest = &self.guest;
        let (lookup, functions) = self.installed().dispatch.resources();
        guest.runtime.with_runtime(|wasm| {
            let (module, guest_code) = match source {
                ModuleSource::Wasm(module) => (module, None),
                ModuleSource::Guest(code) => (code.module(), Some(code)),
            };
            let calls = Calls::new(wasm, guest_code, &guest.resolve_and_jump)?;
            let mut imports = guest.bindings.imports(wasm).to_vec();
            imports.push(wasm.bind(abi::CALL_PROFILE, &self.profiling.memory));
            imports.extend([
                wasm.bind(abi::DISPATCH, &guest.entry_points.dispatch),
                wasm.bind(abi::INTERPRET, &guest.interpret),
                wasm.bind(abi::RESOLVE_CALL, &guest.resolve_call),
            ]);
            imports.extend(guest.io.port_map().imports(wasm));
            imports.extend(guest.io.mmio_map().imports(wasm));
            if let Some(calls) = &calls {
                imports.push(calls.import(wasm));
            }
            imports.extend([
                self.write_code.import(),
                wasm.bind(abi::EXECUTION_ENTRIES, &lookup),
                wasm.bind(abi::EXECUTION_FUNCTIONS, &functions),
            ]);
            let _callback = self.write_code.bind(&mut self.stopped);
            let instance = wasm.instantiate(module, &imports)?;
            Ok(ModuleInstance::new(instance, guest_code.cloned(), calls))
        })
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
    ) -> impl Iterator<Item = &crate::CompiledCode> {
        self.installed()
            .covering(address)
            .filter_map(|module| module.code())
    }
}

impl Cpu {
    pub(crate) fn with_host_turns(mut self, host: HostTurns) -> Self {
        self.instance.code.host_turns = host;
        self
    }

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
