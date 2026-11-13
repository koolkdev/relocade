//! Owns bound Wasm execution, module installation and resources for one CPU.

mod hooks;
mod table;

use super::{
    code, hooks::Handler, CpuContext, Hook, RuntimeMemory, WasmRuntime, WasmRuntimeHandle,
};
use crate::{CpuConfig, FarPointer, IoBus, Memory, Result};
use hooks::Hooks;
use relocade_wasm_runtime::{Function, Module, SharedMemory, WasmBackend, WasmCompiler};
use relocade_x86::{abi, CpuMode, ExecutionExit};
use std::{collections::BTreeMap, rc::Rc};
use table::DispatchTable;

/// An ordinary `() -> ExecutionExit` export to install at a guest linear address.
pub struct DispatchEntry<'a> {
    pub address: u32,
    pub export: &'a str,
}

/// Binds guest execution and installed modules to one runtime and its memory.
/// Runtime borrows stay inside synchronous operations; compilation and host
/// service polling do not hold the runtime.
pub(super) struct CpuInstance {
    runtime: WasmRuntimeHandle,
    pub(super) bindings: RuntimeMemory,
    pub(super) io: IoBus,
    dispatch: Function<WasmRuntime, abi::ExecutionSignature>,
    interpret: Function<WasmRuntime, abi::ExecutionSignature>,
    resolve_call: Function<WasmRuntime, fn() -> u32>,
    step: Function<WasmRuntime, abi::ExecutionSignature>,
    interrupt: Option<Function<WasmRuntime, abi::InterruptSignature>>,
    far_call: Option<Function<WasmRuntime, abi::FarCallSignature>>,
    table: DispatchTable<WasmRuntime>,
    hooks: Hooks,
}

impl CpuInstance {
    pub(super) async fn new(
        memory: Memory,
        cpu: CpuConfig,
        io: IoBus,
        storage: SharedMemory,
    ) -> Result<Self> {
        let runtime = io.attach_cpu()?;
        let bindings =
            runtime.with_runtime(|wasm| RuntimeMemory::new(wasm, memory, cpu, storage))?;
        let target = bindings.configuration().execution_target();
        let (compiler, cache) =
            runtime.with_runtime(|wasm| (wasm.compiler(), wasm.module_cache().cloned()));
        let dispatcher = code::dispatcher(&compiler, target).await?;
        let interpreter = code::interpreter(&compiler, cache.as_ref(), target).await?;
        runtime.with_runtime(|wasm| {
            // Link the dispatcher first; the interpreter imports its entry points.
            let table = DispatchTable::new(wasm)?;
            let mut imports = bindings.imports(wasm).to_vec();
            imports.extend(table.imports(wasm));
            let instance = wasm.instantiate(&dispatcher, &imports)?;
            let dispatch = wasm.function(&instance, abi::DISPATCH)?;
            let interpret = wasm.function(&instance, abi::INTERPRET)?;
            let resolve_call = wasm.function(&instance, abi::RESOLVE_CALL)?;

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
            table.install_interpreter(wasm, &instance)?;
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
                bindings,
                io,
                dispatch,
                interpret,
                resolve_call,
                step,
                interrupt,
                far_call,
                table,
                hooks: Hooks::default(),
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

    pub(super) fn run(&mut self) -> ExecutionExit {
        self.runtime
            .with_runtime(|wasm| self.dispatch.call(wasm, ()))
    }

    pub(super) fn enter_interrupt(&mut self, vector: u8) -> ExecutionExit {
        self.runtime.with_runtime(|wasm| {
            self.interrupt
                .as_ref()
                .expect("real-mode interrupt entry")
                .call(wasm, (vector,))
        })
    }

    pub(super) fn enter_far_call(&mut self, target: FarPointer, stack_bytes: u16) -> ExecutionExit {
        self.runtime.with_runtime(|wasm| {
            self.far_call
                .as_ref()
                .expect("real-mode far-call entry")
                .call(wasm, (target.segment, target.offset, stack_bytes))
        })
    }

    #[cold]
    pub(super) fn step(&mut self) -> ExecutionExit {
        self.runtime.with_runtime(|wasm| self.step.call(wasm, ()))
    }

    pub(super) async fn install(&mut self, hooks: impl IntoIterator<Item = Hook>) -> Result<()> {
        let module = self.hooks.compile(hooks)?;
        self.install_module(&module.bytes, &module.entries())
            .await?;
        self.hooks.install(module);
        Ok(())
    }

    pub(super) async fn install_module(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let compiler = self.runtime.with_runtime(|wasm| wasm.compiler());
        let module = compiler.compile(bytes).await?;
        self.install_compiled_module(&module, entries)
    }

    pub(super) fn install_compiled_module(
        &mut self,
        module: &Module,
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let entries: BTreeMap<_, _> = entries
            .iter()
            .map(|entry| (entry.address, entry.export))
            .collect();
        let entries: Vec<_> = entries
            .into_iter()
            .map(|(address, export)| DispatchEntry { address, export })
            .collect();
        self.runtime.with_runtime(|wasm| {
            let mut imports = self.bindings.imports(wasm).to_vec();
            imports.extend(self.table.imports(wasm));
            imports.extend([
                wasm.bind(abi::DISPATCH, &self.dispatch),
                wasm.bind(abi::INTERPRET, &self.interpret),
                wasm.bind(abi::RESOLVE_CALL, &self.resolve_call),
            ]);
            imports.extend(self.io.port_map().imports(wasm));
            imports.extend(self.io.mmio_map().imports(wasm));
            let instance = wasm.instantiate(module, &imports)?;
            self.table.install(wasm, &instance, &entries)
        })?;
        self.hooks.remove(entries.iter().map(|entry| entry.address));
        Ok(())
    }

    #[inline]
    pub(super) fn handler(&self, id: u32) -> &Rc<dyn Handler> {
        self.hooks.handler(id)
    }
}
