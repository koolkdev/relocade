use super::Backend;
use crate::{Error, Result, WasmTableAccess};
use wasmtime::{Func, Ref, RefType, Store, Table, TableType};

pub(super) fn create<T: 'static>(
    store: &mut Store<T>,
    minimum: u32,
    maximum: Option<u32>,
    initial: Option<&Func>,
) -> Result<Table> {
    Table::new(
        store,
        TableType::new(RefType::FUNCREF, minimum, maximum),
        Ref::Func(initial.copied()),
    )
    .map_err(|error| Error::new("create Wasm table", error))
}

impl<T: 'static> WasmTableAccess for Backend<T> {
    type Backend = Self;

    fn ensure_table_size(
        &mut self,
        table: &Table,
        minimum: u32,
        initial: Option<&Func>,
    ) -> Result<()> {
        let minimum = u64::from(minimum);
        let current = table.size(&self.store);
        if minimum > current {
            table
                .grow(
                    &mut self.store,
                    minimum - current,
                    Ref::Func(initial.copied()),
                )
                .map_err(|error| Error::new("grow Wasm table", error))?;
        }
        Ok(())
    }

    fn set_table_import(&mut self, table: &Table, index: u32, function: &Func) -> Result<()> {
        table
            .set(
                &mut self.store,
                u64::from(index),
                Ref::Func(Some(*function)),
            )
            .map_err(|error| Error::new("set Wasm table function", error))
    }
}
