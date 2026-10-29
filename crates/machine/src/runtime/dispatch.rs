//! Links executable modules and owns dispatch for one CPU.

mod table;

use super::{code, Backend, RuntimeMemory};
use crate::Result;
use relocade_wasm_runtime::{Function, Import, Module, WasmBackend};
use relocade_x86::{abi, ExecutionExit};
use table::DispatchTable;

type InterruptEntry = Function<Backend, fn(u8) -> ExecutionExit>;

/// An ordinary `() -> ExecutionExit` export to publish at a guest linear address.
pub struct DispatchEntry<'a> {
    pub address: u32,
    pub export: &'a str,
}

/// Bound dispatcher, interpreter, host imports and callable entries for one CPU.
/// Keeps module linking and publication together so every entry uses the same
/// runtime bindings and execution table.
pub(crate) struct Dispatch {
    dispatch: Function<Backend, abi::ExecutionSignature>,
    interpret: Function<Backend, abi::ExecutionSignature>,
    interrupt: Option<InterruptEntry>,
    step: Option<Function<Backend, abi::ExecutionSignature>>,
    table: DispatchTable<Backend>,
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
        let mut dispatch = Self::link_dispatcher(backend, bindings, &module, host_imports).await?;
        let module = code::interpreter(backend, target).await?;
        let instance = dispatch.instantiate(backend, bindings, &module).await?;
        dispatch.table.install_interpreter(backend, &instance)?;
        dispatch.step = Some(backend.function(&instance, abi::STEP)?);
        if target.profile == relocade_x86::ExecutionProfile::Real16 {
            dispatch.interrupt = Some(backend.function(&instance, abi::INTERRUPT)?);
        }
        Ok(dispatch)
    }

    async fn link_dispatcher(
        backend: &mut Backend,
        bindings: &RuntimeMemory,
        module: &Module,
        host_imports: Vec<Import<'static, Backend>>,
    ) -> Result<Self> {
        let table = DispatchTable::new(backend)?;
        let mut imports = bindings.imports(backend).to_vec();
        imports.extend(table.imports(backend));
        let instance = backend.instantiate(module, &imports).await?;
        Ok(Self {
            dispatch: backend.function(&instance, abi::DISPATCH)?,
            interpret: backend.function(&instance, abi::INTERPRET)?,
            interrupt: None,
            step: None,
            table,
            host_imports,
        })
    }

    pub(crate) fn run(&self, backend: &mut Backend) -> ExecutionExit {
        self.dispatch.call(backend, ())
    }

    pub(crate) fn step(&self, backend: &mut Backend) -> ExecutionExit {
        self.step
            .as_ref()
            .expect("interpreter step")
            .call(backend, ())
    }

    pub(crate) fn interrupt(&self, backend: &mut Backend, vector: u8) -> ExecutionExit {
        self.interrupt
            .as_ref()
            .expect("real-mode interrupt entry")
            .call(backend, (vector,))
    }

    pub(crate) async fn install(
        &mut self,
        backend: &mut Backend,
        bindings: &RuntimeMemory,
        module: &Module,
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let instance = self.instantiate(backend, bindings, module).await?;
        self.table.install(backend, &instance, entries)
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
