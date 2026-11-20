//! Each installation owns its call table, including unresolved destinations.

use crate::{CompiledCode, Result};
use relocade_wasm_runtime::{
    ExportFunction, Function, FunctionRef, Import, Table, WasmTableAccess,
};
use relocade_x86::abi;

pub(in super::super) struct Calls<B: ExportFunction<abi::ExecutionSignature>> {
    table: Table<B, abi::ExecutionSignature>,
    pub(super) targets: Vec<u32>,
    unresolved: FunctionRef<B, abi::ExecutionSignature>,
}

/// A registered caller retains the exact slot to update when its target changes.
pub(super) struct CallSite<B: ExportFunction<abi::ExecutionSignature>> {
    table: Table<B, abi::ExecutionSignature>,
    index: u32,
    unresolved: FunctionRef<B, abi::ExecutionSignature>,
}

impl<B: ExportFunction<abi::ExecutionSignature>> CallSite<B> {
    pub(super) fn bind(
        &self,
        context: &mut impl WasmTableAccess<Backend = B>,
        target: Option<&FunctionRef<B, abi::ExecutionSignature>>,
    ) -> Result<()> {
        self.table
            .set(context, self.index, target.unwrap_or(&self.unresolved))?;
        Ok(())
    }
}

impl<B: ExportFunction<abi::ExecutionSignature>> Calls<B> {
    pub(in super::super) fn new(
        backend: &mut B,
        code: Option<&CompiledCode>,
        resolver: &Function<B, abi::ExecutionSignature>,
    ) -> Result<Option<Self>> {
        let Some(code) = code else { return Ok(None) };
        let targets = code.call_targets();
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
            targets: targets.to_vec(),
            unresolved,
        }))
    }

    pub(in super::super) fn import<'a>(&'a self, backend: &B) -> Import<'a, B>
    where
        B: 'a,
    {
        backend.bind(abi::CALL_TARGETS, &self.table)
    }

    pub(super) fn sites(&self) -> impl Iterator<Item = (u32, CallSite<B>)> + '_ {
        self.targets.iter().enumerate().map(|(index, &target)| {
            (
                target,
                CallSite {
                    table: self.table.clone(),
                    index: index as u32,
                    unresolved: self.unresolved.clone(),
                },
            )
        })
    }
}

#[cfg(test)]
mod tests;
