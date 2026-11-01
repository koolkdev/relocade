//! Links executable modules and owns dispatch for one CPU.

mod registry;
mod table;

use super::{code, Backend, Entry, Hook, RuntimeMemory};
use crate::Result;
use registry::{Batch, Registry};
use relocade_wasm_runtime::{Function, Import, Module, WasmBackend, WasmCompiler};
use relocade_x86::{abi, ExecutionExit, ExecutionProfile};

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
    step: Function<Backend, abi::ExecutionSignature>,
    interrupt: Option<Function<Backend, fn(u8) -> ExecutionExit>>,
    registry: Registry<Backend>,
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
        let registry = Registry::new(backend)?;
        let mut imports = bindings.imports(backend).to_vec();
        imports.extend(registry.table.imports(backend));
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
        registry.table.install_interpreter(backend, &instance)?;
        Ok(Self {
            dispatch,
            interpret,
            step: backend.function(&instance, abi::STEP)?,
            interrupt: if target.profile == ExecutionProfile::Real16 {
                Some(backend.function(&instance, abi::INTERRUPT)?)
            } else {
                None
            },
            registry,
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

    #[cold]
    pub(crate) fn step(&self, backend: &mut Backend) -> ExecutionExit {
        self.step.call(backend, ())
    }

    pub(crate) async fn install(
        &mut self,
        backend: &mut Backend,
        bindings: &RuntimeMemory,
        entries: impl IntoIterator<Item = Entry>,
    ) -> Result<()> {
        let (program, batch) = self.registry.prepare(entries)?;
        let bytes = program.compile()?;
        let module = backend.compiler().compile(&bytes).await?;
        self.publish(backend, bindings, &module, batch).await
    }

    pub(crate) async fn install_module(
        &mut self,
        backend: &mut Backend,
        bindings: &RuntimeMemory,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let batch = Batch::module(entries);
        let module = backend.compiler().compile(bytes).await?;
        self.publish(backend, bindings, &module, batch).await
    }

    #[inline]
    pub(crate) fn hook(&self, id: u32) -> &Hook {
        self.registry.hook(id)
    }

    async fn publish(
        &mut self,
        backend: &mut Backend,
        bindings: &RuntimeMemory,
        module: &Module,
        batch: Batch,
    ) -> Result<()> {
        let instance = self.instantiate(backend, bindings, module).await?;
        self.registry.publish(backend, &instance, batch)
    }

    async fn instantiate(
        &self,
        backend: &mut Backend,
        bindings: &RuntimeMemory,
        module: &Module,
    ) -> relocade_wasm_runtime::Result<<Backend as WasmBackend>::Instance> {
        let mut imports = bindings.imports(backend).to_vec();
        imports.extend(self.registry.table.imports(backend));
        imports.extend([
            backend.bind(abi::DISPATCH, &self.dispatch),
            backend.bind(abi::INTERPRET, &self.interpret),
        ]);
        imports.extend(self.host_imports.iter().cloned());
        backend.instantiate(module, &imports).await
    }
}
