//! Links executable modules and owns dispatch for one CPU.

mod hooks;
mod table;

use super::{code, hooks::Handler, io, Hook, RuntimeMemory, WasmRuntime, WasmRuntimeHandle};
use crate::{FarPointer, IoBus, Result};
use hooks::Hooks;
use relocade_wasm_runtime::{Function, Import, Module, WasmBackend, WasmCompiler};
use relocade_x86::{abi, ExecutionExit, ExecutionProfile};
use std::{collections::BTreeMap, rc::Rc};
use table::DispatchTable;

/// An ordinary `() -> ExecutionExit` export to install at a guest linear address.
pub struct DispatchEntry<'a> {
    pub address: u32,
    pub export: &'a str,
}

/// Bound dispatcher, interpreter, host imports and callable entries for one CPU.
/// Keeps module linking and installation together so every entry uses the same
/// runtime bindings and execution table.
pub(crate) struct Dispatch {
    dispatch: Function<WasmRuntime, abi::ExecutionSignature>,
    interpret: Function<WasmRuntime, abi::ExecutionSignature>,
    step: Function<WasmRuntime, abi::ExecutionSignature>,
    interrupt: Option<Function<WasmRuntime, abi::InterruptSignature>>,
    far_call: Option<Function<WasmRuntime, abi::FarCallSignature>>,
    table: DispatchTable<WasmRuntime>,
    hooks: Hooks,
    host_imports: Vec<Import<'static, WasmRuntime>>,
}

impl Dispatch {
    pub(crate) async fn new(
        runtime: &WasmRuntimeHandle,
        bindings: &RuntimeMemory,
        bus: &IoBus,
    ) -> Result<Self> {
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

            imports.extend([
                wasm.bind(abi::DISPATCH, &dispatch),
                wasm.bind(abi::INTERPRET, &interpret),
            ]);
            let host_imports = io::imports(wasm, bus);
            imports.extend(host_imports.iter().cloned());
            let instance = wasm.instantiate(&interpreter, &imports)?;
            table.install_interpreter(wasm, &instance)?;
            Ok(Self {
                dispatch,
                interpret,
                step: wasm.function(&instance, abi::STEP)?,
                interrupt: if target.profile == ExecutionProfile::Real16 {
                    Some(wasm.function(&instance, abi::INTERRUPT)?)
                } else {
                    None
                },
                far_call: if target.profile == ExecutionProfile::Real16 {
                    Some(wasm.function(&instance, abi::FAR_CALL)?)
                } else {
                    None
                },
                table,
                hooks: Hooks::default(),
                host_imports,
            })
        })
    }

    pub(crate) fn run(&self, wasm: &mut WasmRuntime) -> ExecutionExit {
        self.dispatch.call(wasm, ())
    }

    pub(crate) fn enter_interrupt(&self, wasm: &mut WasmRuntime, vector: u8) -> ExecutionExit {
        self.interrupt
            .as_ref()
            .expect("real-mode interrupt entry")
            .call(wasm, (vector,))
    }

    pub(crate) fn enter_far_call(
        &self,
        wasm: &mut WasmRuntime,
        target: FarPointer,
        stack_bytes: u16,
    ) -> ExecutionExit {
        self.far_call
            .as_ref()
            .expect("real-mode far-call entry")
            .call(wasm, (target.segment, target.offset, stack_bytes))
    }

    #[cold]
    pub(crate) fn step(&self, wasm: &mut WasmRuntime) -> ExecutionExit {
        self.step.call(wasm, ())
    }

    pub(crate) async fn install(
        &mut self,
        runtime: &WasmRuntimeHandle,
        bindings: &RuntimeMemory,
        hooks: impl IntoIterator<Item = Hook>,
    ) -> Result<()> {
        let module = self.hooks.compile(hooks)?;
        self.install_module(runtime, bindings, &module.bytes, &module.entries())
            .await?;
        self.hooks.install(module);
        Ok(())
    }

    pub(crate) async fn install_module(
        &mut self,
        runtime: &WasmRuntimeHandle,
        bindings: &RuntimeMemory,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let compiler = runtime.with_runtime(|wasm| wasm.compiler());
        let module = compiler.compile(bytes).await?;
        self.install_compiled_module(runtime, bindings, &module, entries)
    }

    pub(crate) fn install_compiled_module(
        &mut self,
        runtime: &WasmRuntimeHandle,
        bindings: &RuntimeMemory,
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
        runtime.with_runtime(|wasm| {
            let instance = self.instantiate(wasm, bindings, module)?;
            self.table.install(wasm, &instance, &entries)
        })?;
        self.hooks.remove(entries.iter().map(|entry| entry.address));
        Ok(())
    }

    #[inline]
    pub(super) fn handler(&self, id: u32) -> &Rc<dyn Handler> {
        self.hooks.handler(id)
    }

    fn instantiate(
        &self,
        wasm: &mut WasmRuntime,
        bindings: &RuntimeMemory,
        module: &Module,
    ) -> relocade_wasm_runtime::Result<<WasmRuntime as WasmBackend>::Instance> {
        let mut imports = bindings.imports(wasm).to_vec();
        imports.extend(self.table.imports(wasm));
        imports.extend([
            wasm.bind(abi::DISPATCH, &self.dispatch),
            wasm.bind(abi::INTERPRET, &self.interpret),
        ]);
        imports.extend(self.host_imports.iter().cloned());
        wasm.instantiate(module, &imports)
    }
}
