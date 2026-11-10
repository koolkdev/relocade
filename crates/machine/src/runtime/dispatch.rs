//! Links executable modules and owns dispatch for one CPU.

mod hooks;
mod table;

use super::{code, hooks::Handler, Backend, Hook, RuntimeMemory};
use crate::{FarPointer, Result};
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
    dispatch: Function<Backend, abi::ExecutionSignature>,
    interpret: Function<Backend, abi::ExecutionSignature>,
    step: Function<Backend, abi::ExecutionSignature>,
    interrupt: Option<Function<Backend, abi::InterruptSignature>>,
    far_call: Option<Function<Backend, abi::FarCallSignature>>,
    table: DispatchTable<Backend>,
    hooks: Hooks,
    host_imports: Vec<Import<'static, Backend>>,
}

impl Dispatch {
    pub(crate) async fn new(
        backend: &mut Backend,
        bindings: &RuntimeMemory,
        host_imports: Vec<Import<'static, Backend>>,
    ) -> Result<Self> {
        let target = bindings.configuration().execution_target();
        let module = code::dispatcher(backend, target).await?;
        // Link the dispatcher first; the interpreter imports its entry points.
        let table = DispatchTable::new(backend)?;
        let mut imports = bindings.imports(backend).to_vec();
        imports.extend(table.imports(backend));
        let instance = backend.instantiate(&module, &imports).await?;
        let dispatch = backend.function(&instance, abi::DISPATCH)?;
        let interpret = backend.function(&instance, abi::INTERPRET)?;

        imports.extend([
            backend.bind(abi::DISPATCH, &dispatch),
            backend.bind(abi::INTERPRET, &interpret),
        ]);
        imports.extend(host_imports.iter().cloned());
        let module = code::interpreter(backend, target).await?;
        let instance = backend.instantiate(&module, &imports).await?;
        table.install_interpreter(backend, &instance)?;
        Ok(Self {
            dispatch,
            interpret,
            step: backend.function(&instance, abi::STEP)?,
            interrupt: if target.profile == ExecutionProfile::Real16 {
                Some(backend.function(&instance, abi::INTERRUPT)?)
            } else {
                None
            },
            far_call: if target.profile == ExecutionProfile::Real16 {
                Some(backend.function(&instance, abi::FAR_CALL)?)
            } else {
                None
            },
            table,
            hooks: Hooks::default(),
            host_imports,
        })
    }

    pub(crate) fn run(&self, backend: &mut Backend) -> ExecutionExit {
        self.dispatch.call(backend, ())
    }

    pub(crate) fn enter_interrupt(&self, backend: &mut Backend, vector: u8) -> ExecutionExit {
        self.interrupt
            .as_ref()
            .expect("real-mode interrupt entry")
            .call(backend, (vector,))
    }

    pub(crate) fn enter_far_call(
        &self,
        backend: &mut Backend,
        target: FarPointer,
        stack_bytes: u16,
    ) -> ExecutionExit {
        self.far_call
            .as_ref()
            .expect("real-mode far-call entry")
            .call(backend, (target.segment, target.offset, stack_bytes))
    }

    #[cold]
    pub(crate) fn step(&self, backend: &mut Backend) -> ExecutionExit {
        self.step.call(backend, ())
    }

    pub(crate) async fn install(
        &mut self,
        backend: &mut Backend,
        bindings: &RuntimeMemory,
        hooks: impl IntoIterator<Item = Hook>,
    ) -> Result<()> {
        let module = self.hooks.compile(hooks)?;
        self.install_module(backend, bindings, &module.bytes, &module.entries())
            .await?;
        self.hooks.install(module);
        Ok(())
    }

    pub(crate) async fn install_module(
        &mut self,
        backend: &mut Backend,
        bindings: &RuntimeMemory,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let module = backend.compiler().compile(bytes).await?;
        self.install_compiled_module(backend, bindings, &module, entries)
            .await
    }

    pub(crate) async fn install_compiled_module(
        &mut self,
        backend: &mut Backend,
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
        let instance = self.instantiate(backend, bindings, module).await?;
        self.table.install(backend, &instance, &entries)?;
        self.hooks.remove(entries.iter().map(|entry| entry.address));
        Ok(())
    }

    #[inline]
    pub(super) fn handler(&self, id: u32) -> &Rc<dyn Handler> {
        self.hooks.handler(id)
    }

    async fn instantiate(
        &self,
        backend: &mut Backend,
        bindings: &RuntimeMemory,
        module: &Module,
    ) -> relocade_wasm_runtime::Result<<Backend as WasmBackend>::Instance> {
        let mut imports = bindings.imports(backend).to_vec();
        imports.extend(self.table.imports(backend));
        imports.extend([
            backend.bind(abi::DISPATCH, &self.dispatch),
            backend.bind(abi::INTERPRET, &self.interpret),
        ]);
        imports.extend(self.host_imports.iter().cloned());
        backend.instantiate(module, &imports).await
    }
}
