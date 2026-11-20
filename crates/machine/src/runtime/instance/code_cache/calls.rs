//! Each installation owns its call table, including unresolved destinations.

use crate::{runtime::WasmRuntime, CompiledCode, Result};
use relocade_wasm_runtime::{Function, FunctionRef, Import, Table, WasmBackend, WasmTableAccess};
use relocade_x86::abi;

pub(super) struct Calls {
    table: Table<WasmRuntime, abi::ExecutionSignature>,
    pub(super) targets: Vec<u32>,
    unresolved: FunctionRef<WasmRuntime, abi::ExecutionSignature>,
}

/// A registered caller retains the exact slot to update when its target changes.
pub(super) struct CallSite {
    table: Table<WasmRuntime, abi::ExecutionSignature>,
    index: u32,
    unresolved: FunctionRef<WasmRuntime, abi::ExecutionSignature>,
}

impl CallSite {
    pub(super) fn bind(
        &self,
        context: &mut impl WasmTableAccess<Backend = WasmRuntime>,
        target: Option<&FunctionRef<WasmRuntime, abi::ExecutionSignature>>,
    ) -> Result<()> {
        self.table
            .set(context, self.index, target.unwrap_or(&self.unresolved))?;
        Ok(())
    }
}

impl Calls {
    pub(super) fn new(
        wasm: &mut WasmRuntime,
        code: Option<&CompiledCode>,
        resolver: &Function<WasmRuntime, abi::ExecutionSignature>,
    ) -> Result<Option<Self>> {
        let Some(code) = code else { return Ok(None) };
        let targets = code.call_targets();
        if targets.is_empty() {
            return Ok(None);
        }
        let unresolved = wasm.import_function(resolver);
        let table = wasm.create_table(abi::CALL_TARGETS, None)?;
        table.ensure_size(
            wasm,
            u32::try_from(targets.len()).expect("call table fits u32"),
            Some(&unresolved),
        )?;
        Ok(Some(Self {
            table,
            targets: targets.to_vec(),
            unresolved,
        }))
    }

    pub(super) fn import<'a>(&'a self, wasm: &WasmRuntime) -> Import<'a, WasmRuntime> {
        wasm.bind(abi::CALL_TARGETS, &self.table)
    }

    pub(super) fn sites(&self) -> impl Iterator<Item = (u32, CallSite)> + '_ {
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
