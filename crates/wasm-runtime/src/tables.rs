use crate::{FunctionRef, Result, WasmBackend};
use relocade_compiler::abi::FunctionSignature;
use std::marker::PhantomData;

/// A live table whose entries share one logical function signature.
/// Clones refer to the same slots. Linked instances observe replacements without
/// being relinked. Access requires the backend that created the table or one of
/// its scoped contexts. Table/function handles may be kept in backend-local state;
/// callbacks borrow that state through their context rather than capturing it.
/// Import declarations must preserve its logical signature:
/// ```compile_fail
/// use relocade_compiler::abi;
/// use relocade_wasm_runtime::{Backend, Table, WasmBackend};
/// fn wrong(backend: &Backend, table: &Table<Backend, fn(u16)>) {
///     backend.bind(abi::Table::<fn(u32)>::new("test", "wide", 1), table);
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
    /// guest invocation. Unrelated slots are unchanged. Callback state still
    /// belongs to the backend; retaining a table does not extend its lifetime.
    /// Logical widths must match even when Wasm uses the same carrier:
    /// ```compile_fail
    /// use relocade_wasm_runtime::{Backend, FunctionRef, Table};
    /// fn wrong(backend: &mut Backend, table: &Table<Backend, fn(u16)>,
    ///          function: &FunctionRef<Backend, fn(u32)>) {
    ///     table.set(backend, 0, function);
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

/// Access to the live function tables of one backend, either while stopped or
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
