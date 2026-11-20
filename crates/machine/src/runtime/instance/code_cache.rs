//! Compiled code and automatic compilation for one CPU.

use super::{
    hooks::PreparedHandler,
    modules::{Calls, CpuModules, ModuleInstance},
    profiling::Profiling,
    DispatchEntry, GuestRuntime, WasmRuntime,
};
use crate::{
    runtime::{hooks::Handler, memory::HostWritePages, CodeWork},
    worker::HostTurns,
    CompilationBinding, CompiledCode, CompilerClient, FarPointer, Hook, Result,
};
use relocade_wasm_runtime::{Module, WasmBackend, WasmCompiler};
use relocade_x86::abi;
use std::{ops::Range, rc::Rc, task::Context};

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
    pub(super) modules: CpuModules,
    pub(super) profiling: Profiling,
    pub(super) compiler: Option<CompilerClient>,
    pub(super) pending: Vec<FarPointer>,
    pub(super) host_turns: HostTurns,
    pub(super) guest: Rc<GuestRuntime>,
}

impl CodeCache {
    pub(super) fn new(guest: Rc<GuestRuntime>, modules: CpuModules, profiling: Profiling) -> Self {
        Self {
            host_writes: HostWritePages::new(&guest.runtime, &guest.bindings),
            modules,
            profiling,
            compiler: None,
            pending: Vec::new(),
            host_turns: HostTurns::default(),
            guest,
        }
    }

    pub(super) fn enter_guest(
        &mut self,
        context: &mut relocade_wasm_runtime::Context<'_>,
    ) -> impl Drop + '_ {
        self.modules.enter_guest(context)
    }

    pub(in crate::runtime) fn handler(&self, id: u32) -> &Rc<dyn Handler> {
        self.modules.hooks.handler(id)
    }

    /// Apply invalidations before publishing replies so a recompiled entry can use its slot.
    pub(in crate::runtime) fn service(&mut self, ready: CodeWork, cx: &mut Context<'_>) {
        if ready.retirements() {
            self.guest
                .runtime
                .with_runtime(|wasm| self.modules.retire_queued(wasm))
                .expect("retire modified code");
        }
        if ready.compiler_reply() {
            self.poll_compiler_replies(cx);
        }
    }

    async fn compile_wasm(&self, bytes: &[u8]) -> Result<Module> {
        let compiler = self.guest.runtime.with_runtime(|wasm| wasm.compiler());
        Ok(self.host_turns.wait(compiler.compile(bytes)).await?)
    }

    pub(in crate::runtime) async fn install(
        &mut self,
        hooks: impl IntoIterator<Item = Hook>,
    ) -> Result<()> {
        let mut hooks = self.modules.hooks.compile(hooks)?;
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
        // A host write may have invalidated a binding before the worker replied.
        if self.guest.bindings.guest.code_wake().take_retirements() {
            self.guest
                .runtime
                .with_runtime(|wasm| self.modules.retire_queued(wasm))?;
        }
        let mut exports = Vec::new();
        let mut fallbacks = Vec::new();
        for binding in code.bindings() {
            match binding {
                CompilationBinding::Export { entry, export } => {
                    let key = entry.packed();
                    if matches!(installation, Installation::Explicit)
                        || !self.modules.has_entry(key)
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
        let (id, installed) = self
            .guest
            .runtime
            .with_runtime(|wasm| self.modules.register_exports(wasm, instance, entries))?;
        if let Some((source, mut registry)) = checked_source {
            installed.track_source(registry.register(id, source), &self.host_writes);
        }
        // Releasing a host callback can reenter the runtime or write guest memory.
        self.modules.hooks.replace_bindings(entries, handlers);
        Ok(())
    }

    fn instantiate_module(
        &mut self,
        source: ModuleSource<'_>,
    ) -> Result<ModuleInstance<WasmRuntime>> {
        let guest = &self.guest;
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
            let instance = self.modules.instantiate(wasm, module, imports)?;
            Ok(ModuleInstance::new(instance, guest_code.cloned(), calls))
        })
    }

    pub(in crate::runtime) fn invalidate_code(&mut self, range: Range<u64>) -> Result<()> {
        self.guest
            .runtime
            .with_runtime(|wasm| self.modules.invalidate(&mut wasm.context(), range))
    }

    pub(in crate::runtime) fn compiled_modules_at(
        &self,
        address: u32,
    ) -> impl Iterator<Item = &crate::CompiledCode> {
        self.modules
            .covering(address)
            .filter_map(|module| module.code())
    }

    pub(super) fn bind_interpreter_if_vacant(
        &mut self,
        entries: impl IntoIterator<Item = FarPointer>,
    ) -> Result<()> {
        self.guest
            .runtime
            .with_runtime(|wasm| self.modules.bind_interpreter_if_vacant(wasm, entries))
    }
}
