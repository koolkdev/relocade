//! Links executable modules and owns dispatch for one CPU.

mod table;

use super::{code, Runtime};
use crate::{CpuBindings, Error, Result};
use relocade_wasm_runtime::{Import, WasmBackend};
use relocade_x86::{abi, ExecutionExit};
use table::DispatchTable;

#[derive(Clone, Copy)]
pub(crate) enum EntryKind {
    Generated,
    Hook,
}

/// An ordinary `() -> i64` export to publish at a guest linear address.
pub struct DispatchEntry<'a> {
    pub address: u32,
    pub export: &'a str,
}

/// Bound dispatcher, interpreter, host imports and callable entries for one CPU.
/// Keeps module linking and publication together so every entry uses the same
/// runtime bindings and execution table.
pub(crate) struct Dispatch<B: WasmBackend> {
    dispatch: B::Function,
    interpret: B::Function,
    table: DispatchTable<B>,
    host_imports: Vec<Import<'static, B>>,
}

impl<B: WasmBackend> Dispatch<B> {
    pub(crate) async fn new(
        backend: &mut B,
        bindings: &CpuBindings<B::Memory>,
        runtime: &Runtime,
        host_imports: Vec<Import<'static, B>>,
    ) -> Result<Self> {
        let target = bindings.configuration().execution_target();
        let module = runtime.dispatcher(backend, target).await?;
        // Link the dispatcher first; the interpreter imports its entry points.
        let dispatch = Self::link_dispatcher(backend, bindings, &module, host_imports).await?;
        let module = code::interpreter(backend, target).await?;
        let instance = dispatch
            .instantiate(backend, bindings, &module, &[])
            .await?;
        dispatch.table.install_interpreter(backend, &instance)?;
        Ok(dispatch)
    }

    async fn link_dispatcher(
        backend: &mut B,
        bindings: &CpuBindings<B::Memory>,
        module: &B::Module,
        host_imports: Vec<Import<'static, B>>,
    ) -> Result<Self> {
        let table = DispatchTable::new(backend)?;
        let mut imports = bindings.imports(backend).to_vec();
        imports.extend(table.imports(backend));
        let instance = backend.instantiate(module, &imports).await?;
        Ok(Self {
            dispatch: backend.function(&instance, abi::DISPATCH)?,
            interpret: backend.function(&instance, abi::INTERPRET)?,
            table,
            host_imports,
        })
    }

    pub(crate) fn run(
        &mut self,
        backend: &mut B,
        bindings: &CpuBindings<B::Memory>,
        target: u64,
    ) -> Result<ExecutionExit> {
        let generation = bindings.mapping_generation(backend);
        self.table.synchronize(backend, generation);
        bindings.set_instruction_target(backend, target);
        let word = backend.call(&self.dispatch)? as u64;
        ExecutionExit::from_bits(word).ok_or(Error::InvalidExit(word))
    }

    pub(crate) async fn install(
        &mut self,
        backend: &mut B,
        bindings: &CpuBindings<B::Memory>,
        module: &B::Module,
        additional: &[Import<'_, B>],
        entries: &[DispatchEntry<'_>],
        kind: EntryKind,
    ) -> Result<()> {
        let generation = bindings.mapping_generation(backend);
        self.table.synchronize(backend, generation);
        let instance = self
            .instantiate(backend, bindings, module, additional)
            .await?;
        self.table.install(backend, &instance, entries, kind)
    }

    async fn instantiate(
        &self,
        backend: &mut B,
        bindings: &CpuBindings<B::Memory>,
        module: &B::Module,
        additional: &[Import<'_, B>],
    ) -> Result<B::Instance> {
        let mut imports = bindings.imports(backend).to_vec();
        imports.extend(self.table.imports(backend));
        imports.extend([
            backend.bind(abi::DISPATCH, &self.dispatch),
            backend.bind(abi::INTERPRET, &self.interpret),
        ]);
        imports.extend(self.host_imports.iter().cloned());
        imports.extend(additional.iter().cloned());
        backend
            .instantiate(module, &imports)
            .await
            .map_err(Into::into)
    }
}
