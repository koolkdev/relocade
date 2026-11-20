//! Owns bound Wasm execution, module installation and resources for one CPU.

mod hooks;
mod modules;
mod profiling;

use super::{
    code, hooks::Handler, memory::HostWritePages, CpuContext, Hook, RuntimeMemory, WasmRuntime,
    WasmRuntimeHandle,
};
use crate::{memory::CpuBinding, CompiledCode, FarPointer, IoBus, Result};
use hooks::PreparedHandler;
use modules::{Calls, ModuleInstance, Modules};
use profiling::Profiling;
use relocade_wasm_runtime::{Function, Module, SharedMemory, WasmBackend, WasmCompiler};
use relocade_x86::{abi, CpuMode, ExecutionExit};
use std::{ops::Range, rc::Rc};

/// An ordinary `() -> ExecutionExit` export at a packed real-mode CS:IP key.
pub struct DispatchEntry<'a> {
    pub key: u32,
    pub export: &'a str,
}

enum ModuleSource<'a> {
    Wasm(&'a Module),
    Guest(&'a CompiledCode),
}

/// Wasm exports through which the host can enter the guest.
struct GuestEntryPoints {
    dispatch: Function<WasmRuntime, abi::ExecutionSignature>,
    step: Function<WasmRuntime, abi::ExecutionSignature>,
    interrupt: Option<Function<WasmRuntime, abi::InterruptSignature>>,
    far_call: Option<Function<WasmRuntime, abi::FarCallSignature>>,
}

/// Binds guest execution and installed modules to one runtime and its memory.
/// Runtime borrows stay inside synchronous operations; compilation and host
/// service polling do not hold the runtime.
pub(super) struct CpuInstance {
    runtime: WasmRuntimeHandle,
    // Discard the private routes before dropping the modules that hold them.
    host_writes: Rc<HostWritePages>,
    // Retire source registrations before a standalone CPU releases its memory attachment.
    modules: Modules<WasmRuntime>,
    pub(super) bindings: RuntimeMemory,
    pub(super) io: IoBus,
    entry_points: GuestEntryPoints,
    interpret: Function<WasmRuntime, abi::ExecutionSignature>,
    resolve_call: Function<WasmRuntime, fn() -> (u32, bool)>,
    resolve_and_jump: Function<WasmRuntime, abi::ExecutionSignature>,
    pub(super) profiling: Profiling,
}

impl CpuInstance {
    pub(super) async fn new(binding: CpuBinding, io: IoBus, storage: SharedMemory) -> Result<Self> {
        let runtime = io.attach_cpu()?;
        let bindings = runtime.with_runtime(|wasm| RuntimeMemory::new(wasm, binding, storage))?;
        let target = bindings.configuration().execution_target();
        let (compiler, cache) =
            runtime.with_runtime(|wasm| (wasm.compiler(), wasm.module_cache().cloned()));
        let dispatcher = code::dispatcher(&compiler, target).await?;
        let interpreter = code::interpreter(&compiler, cache.as_ref(), target).await?;
        runtime.with_runtime(|wasm| {
            // Link the dispatcher first; the interpreter imports its entry points.
            let mut modules = Modules::new(wasm)?;
            let profiling = Profiling::new(wasm, target)?;
            let mut imports = bindings.imports(wasm).to_vec();
            imports.extend(modules.imports(wasm));
            imports.push(wasm.bind(abi::CALL_PROFILE, &profiling.memory));
            let instance = wasm.instantiate(&dispatcher, &imports)?;
            let dispatch = wasm.function(&instance, abi::DISPATCH)?;
            let interpret = wasm.function(&instance, abi::INTERPRET)?;
            let resolve_call = wasm.function(&instance, abi::RESOLVE_CALL)?;
            let resolve_and_jump = wasm.function(&instance, abi::RESOLVE_AND_JUMP)?;

            imports.extend([
                wasm.bind(abi::DISPATCH, &dispatch),
                wasm.bind(abi::INTERPRET, &interpret),
                wasm.bind(abi::RESOLVE_CALL, &resolve_call),
            ]);
            io.port_map().initialize(wasm, &io.downgrade().0)?;
            imports.extend(io.port_map().imports(wasm));
            io.mmio_map().initialize(
                wasm,
                &io.downgrade().0,
                &bindings.mapping,
                bindings.guest.configuration().memory_map(),
            )?;
            imports.extend(io.mmio_map().imports(wasm));
            let instance = wasm.instantiate(&interpreter, &imports)?;
            modules.install_interpreter(wasm, &instance)?;
            let step = wasm.function(&instance, abi::STEP)?;
            let interrupt = if bindings.cpu.mode() == CpuMode::Real {
                Some(wasm.function(&instance, abi::INTERRUPT)?)
            } else {
                None
            };
            let far_call = if bindings.cpu.mode() == CpuMode::Real {
                Some(wasm.function(&instance, abi::FAR_CALL)?)
            } else {
                None
            };
            Ok(Self {
                runtime: runtime.clone(),
                host_writes: HostWritePages::new(&runtime, &bindings),
                bindings,
                io,
                entry_points: GuestEntryPoints {
                    dispatch,
                    step,
                    interrupt,
                    far_call,
                },
                interpret,
                resolve_call,
                resolve_and_jump,
                modules,
                profiling,
            })
        })
    }

    pub(super) fn context(&mut self) -> CpuContext<'_> {
        CpuContext {
            cpu: &self.bindings.cpu,
            memory: self.bindings.guest.view().with_io(&self.io),
            io: &self.io,
            guest_calls: None,
        }
    }

    /// Retire code patched by a stopped host before the guest can follow a cached link.
    fn enter_guest(
        &mut self,
        enter: impl FnOnce(&GuestEntryPoints, &mut WasmRuntime) -> ExecutionExit,
    ) -> ExecutionExit {
        self.runtime.with_runtime(|wasm| {
            if self.bindings.guest.take_retirement_request() {
                self.modules
                    .retire_modified(&mut wasm.context())
                    .expect("retire modified code");
            }
            enter(&self.entry_points, wasm)
        })
    }

    pub(super) fn run(&mut self) -> ExecutionExit {
        loop {
            let exit = self.enter_guest(|entries, wasm| entries.dispatch.call(wasm, ()));
            if exit != ExecutionExit::CallProfileFull {
                return exit;
            }
            self.runtime
                .with_runtime(|wasm| self.profiling.collect(wasm, &self.modules));
        }
    }

    pub(super) fn enter_interrupt(&mut self, vector: u8) -> ExecutionExit {
        self.enter_guest(|entries, wasm| {
            entries
                .interrupt
                .as_ref()
                .expect("real-mode interrupt entry")
                .call(wasm, (vector,))
        })
    }

    pub(super) fn enter_far_call(&mut self, target: FarPointer, stack_bytes: u16) -> ExecutionExit {
        self.enter_guest(|entries, wasm| {
            entries
                .far_call
                .as_ref()
                .expect("real-mode far-call entry")
                .call(wasm, (target.segment, target.offset, stack_bytes))
        })
    }

    #[cold]
    pub(super) fn step(&mut self) -> ExecutionExit {
        self.enter_guest(|entries, wasm| entries.step.call(wasm, ()))
    }

    pub(super) async fn install(&mut self, hooks: impl IntoIterator<Item = Hook>) -> Result<()> {
        let mut hooks = self.modules.hooks.compile(hooks)?;
        let module = self.compile_wasm(&hooks.bytes).await?;
        let handlers = hooks.take_handlers();
        self.install_module(ModuleSource::Wasm(&module), &hooks.entries(), handlers)
    }

    pub(super) async fn install_wasm_bytes(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let module = self.compile_wasm(bytes).await?;
        self.install_wasm_module(&module, entries)
    }

    async fn compile_wasm(&self, bytes: &[u8]) -> Result<Module> {
        let compiler = self.runtime.with_runtime(|wasm| wasm.compiler());
        Ok(compiler.compile(bytes).await?)
    }

    pub(super) fn install_wasm_module(
        &mut self,
        module: &Module,
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        self.install_module(ModuleSource::Wasm(module), entries, Vec::new())
    }

    pub(super) fn install_guest_code(
        &mut self,
        code: &CompiledCode,
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        self.install_module(ModuleSource::Guest(code), entries, Vec::new())
    }

    fn install_module(
        &mut self,
        source: ModuleSource<'_>,
        entries: &[DispatchEntry<'_>],
        handlers: Vec<PreparedHandler>,
    ) -> Result<()> {
        let checked_source = match source {
            ModuleSource::Wasm(_) => None,
            ModuleSource::Guest(code) => {
                let source = code.source();
                Some((source, source.lock_current_bytes(&self.bindings.guest)?))
            }
        };
        let instance = self.instantiate_module(source)?;
        let installed = self
            .runtime
            .with_runtime(|wasm| self.modules.register_exports(wasm, instance, entries))?;
        if let Some((source, mut registry)) = checked_source {
            installed.track_source(registry.register(source), &self.host_writes);
        }
        // Releasing a host callback can reenter the runtime or write guest memory.
        self.modules.hooks.replace_bindings(entries, handlers);
        Ok(())
    }

    fn instantiate_module(&self, source: ModuleSource<'_>) -> Result<ModuleInstance<WasmRuntime>> {
        self.runtime.with_runtime(|wasm| {
            let (module, guest_code) = match source {
                ModuleSource::Wasm(module) => (module, None),
                ModuleSource::Guest(code) => (code.module(), Some(code)),
            };
            let calls = Calls::new(wasm, guest_code, &self.resolve_and_jump)?;
            let mut imports = self.bindings.imports(wasm).to_vec();
            imports.extend(self.modules.imports(wasm));
            imports.push(wasm.bind(abi::CALL_PROFILE, &self.profiling.memory));
            imports.extend([
                wasm.bind(abi::DISPATCH, &self.entry_points.dispatch),
                wasm.bind(abi::INTERPRET, &self.interpret),
                wasm.bind(abi::RESOLVE_CALL, &self.resolve_call),
            ]);
            imports.extend(self.io.port_map().imports(wasm));
            imports.extend(self.io.mmio_map().imports(wasm));
            if let Some(calls) = &calls {
                imports.push(calls.import(wasm));
            }
            let instance = wasm.instantiate(module, &imports)?;
            Ok(ModuleInstance::new(instance, guest_code.cloned(), calls))
        })
    }

    pub(super) fn has_entry(&self, key: u32) -> bool {
        self.modules.has_entry(key)
    }

    pub(super) fn invalidate_code(&mut self, range: Range<u64>) -> Result<()> {
        self.runtime
            .with_runtime(|wasm| self.modules.invalidate(&mut wasm.context(), range))
    }

    pub(super) fn compiled_modules_at(
        &self,
        address: u32,
    ) -> impl Iterator<Item = &crate::CompiledCode> {
        self.modules
            .covering(address)
            .filter_map(|module| module.code())
    }

    pub(super) fn install_interpreter_fallback(&mut self, entry: FarPointer) -> Result<()> {
        self.runtime
            .with_runtime(|wasm| self.modules.install_interpreter_fallback(wasm, entry))
    }

    #[inline]
    pub(super) fn handler(&self, id: u32) -> &Rc<dyn Handler> {
        self.modules.hooks.handler(id)
    }
}
