use crate::{FunctionRef, Result, WasmBackend};
use relocade_compiler::abi::FunctionSignature;
use std::marker::PhantomData;

/// A live table whose entries share one logical function signature.
/// Clones refer to the same slots. Linked instances observe replacements without
/// being relinked. Access requires the runtime that created the table or one of
/// its scoped contexts. Callbacks capture the handles they need; use weak
/// references to application owners that also retain those callbacks.
/// Import declarations must preserve its logical signature:
/// ```compile_fail
/// use relocade_compiler::abi;
/// use relocade_wasm_runtime::{WasmRuntime, Table, WasmBackend};
/// fn wrong(runtime: &WasmRuntime, table: &Table<WasmRuntime, fn(u16)>) {
///     runtime.bind(abi::Table::<fn(u32)>::new("test", "wide", 1), table);
/// }
/// ```
pub struct Table<B: WasmBackend, S: FunctionSignature> {
    pub(crate) handle: B::Table,
    signature: PhantomData<fn() -> S>,
}

impl<B: WasmBackend, S: FunctionSignature> Clone for Table<B, S> {
    fn clone(&self) -> Self {
        Self::new(self.handle.clone())
    }
}

impl<B: WasmBackend, S: FunctionSignature> Table<B, S> {
    pub(crate) fn new(handle: B::Table) -> Self {
        Self {
            handle,
            signature: PhantomData,
        }
    }

    /// Replaces an existing slot without changing the table's size.
    /// Replacement during a callback leaves that invocation alive, including its
    /// captures. The next indirect call observes the new entry, even in the same
    /// guest invocation. Unrelated slots are unchanged. Callbacks retain their
    /// explicit captures for as long as the engine keeps their function alive.
    /// Logical widths must match even when Wasm uses the same carrier:
    /// ```compile_fail
    /// use relocade_wasm_runtime::{WasmRuntime, FunctionRef, Table};
    /// fn wrong(runtime: &mut WasmRuntime, table: &Table<WasmRuntime, fn(u16)>,
    ///          function: &FunctionRef<WasmRuntime, fn(u32)>) {
    ///     table.set(runtime, 0, function);
    /// }
    /// ```
    pub fn set(
        &self,
        access: &mut impl WasmTableAccess<Backend = B>,
        index: u32,
        function: &FunctionRef<B, S>,
    ) -> Result<()> {
        access.set_table_import(&self.handle, index, &function.handle)
    }

    /// Grows to at least `minimum` entries, up to the declared maximum.
    /// Existing entries are preserved. New entries share `initial`, or contain
    /// null when it is absent. A smaller minimum leaves the table unchanged.
    pub fn ensure_size(
        &self,
        access: &mut impl WasmTableAccess<Backend = B>,
        minimum: u32,
        initial: Option<&FunctionRef<B, S>>,
    ) -> Result<()> {
        access.ensure_table_size(
            &self.handle,
            minimum,
            initial.map(|function| &function.handle),
        )
    }
}

/// Access to the live function tables of one runtime, either while stopped or
/// from a callback context while the guest is suspended. Mutation finishes before
/// execution resumes; it does not reenter guest code or require relinking.
/// Consumers use the typed operations on [`Table`].
pub trait WasmTableAccess {
    type Backend: WasmBackend;

    #[doc(hidden)]
    fn set_table_import(
        &mut self,
        table: &<Self::Backend as WasmBackend>::Table,
        index: u32,
        function: &<Self::Backend as WasmBackend>::ImportFunction,
    ) -> Result<()>;

    #[doc(hidden)]
    fn ensure_table_size(
        &mut self,
        table: &<Self::Backend as WasmBackend>::Table,
        minimum: u32,
        initial: Option<&<Self::Backend as WasmBackend>::ImportFunction>,
    ) -> Result<()>;
}
