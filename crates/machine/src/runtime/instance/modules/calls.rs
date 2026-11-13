//! Each installation owns its call table, including unresolved destinations.

use super::DispatchTable;
use crate::{CompiledCode, Result};
use relocade_wasm_runtime::{ExportFunction, Function, FunctionRef, Import, Table};
use relocade_x86::abi;

pub(in super::super) struct Calls<B: ExportFunction<abi::ExecutionSignature>> {
    table: Table<B, abi::ExecutionSignature>,
    pub(super) targets: Vec<u32>,
    unresolved: FunctionRef<B, abi::ExecutionSignature>,
}

impl<B: ExportFunction<abi::ExecutionSignature>> Calls<B> {
    pub(in super::super) fn new(
        backend: &mut B,
        code: Option<&CompiledCode>,
        resolver: &Function<B, abi::ExecutionSignature>,
    ) -> Result<Option<Self>> {
        let Some(code) = code else { return Ok(None) };
        let targets = code.call_targets()?;
        if targets.is_empty() {
            return Ok(None);
        }
        let unresolved = backend.import_function(resolver);
        let table = backend.create_table(abi::CALL_TARGETS, None)?;
        table.ensure_size(
            backend,
            u32::try_from(targets.len()).expect("call table fits u32"),
            Some(&unresolved),
        )?;
        Ok(Some(Self {
            table,
            targets,
            unresolved,
        }))
    }

    pub(in super::super) fn import<'a>(&'a self, backend: &B) -> Import<'a, B>
    where
        B: 'a,
    {
        backend.bind(abi::CALL_TARGETS, &self.table)
    }

    pub(super) fn bind(
        &self,
        backend: &mut B,
        dispatch: &DispatchTable<B>,
        index: u32,
    ) -> Result<()> {
        let target = self.targets[index as usize];
        self.table.set(
            backend,
            index,
            dispatch.resolve(target).unwrap_or(&self.unresolved),
        )?;
        Ok(())
    }
}
