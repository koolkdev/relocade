use super::{js_error, set, Context, WasmRuntime};
use crate::{Result, WasmBackend, WasmTableAccess};
use js_sys::{Function, Object, WebAssembly::Table};
use wasm_bindgen::JsValue;

pub(super) fn create(
    minimum: u32,
    maximum: Option<u32>,
    initial: Option<&Function>,
) -> Result<Table> {
    let descriptor = Object::new();
    set(&descriptor, "initial", &minimum.into())?;
    set(&descriptor, "element", &"anyfunc".into())?;
    if let Some(maximum) = maximum {
        set(&descriptor, "maximum", &maximum.into())?;
    }
    Table::new_with_value(
        &descriptor,
        initial.map_or(JsValue::NULL, |value| value.clone().into()),
    )
    .map_err(|error| js_error("create Wasm table", error))
}

impl WasmTableAccess for WasmRuntime {
    type Backend = Self;

    fn ensure_table_size(
        &mut self,
        table: &Table,
        minimum: u32,
        initial: Option<&Function>,
    ) -> Result<()> {
        self.context().ensure_table_size(table, minimum, initial)
    }

    fn set_table_import(&mut self, table: &Table, index: u32, function: &Function) -> Result<()> {
        self.context().set_table_import(table, index, function)
    }
}

impl WasmTableAccess for Context<'_> {
    type Backend = WasmRuntime;

    fn ensure_table_size(
        &mut self,
        table: &Table,
        minimum: u32,
        initial: Option<&Function>,
    ) -> Result<()> {
        let current = table.length();
        if minimum > current {
            table
                .grow_with_value(
                    minimum - current,
                    initial.map_or(JsValue::NULL, |value| value.clone().into()),
                )
                .map_err(|error| js_error("grow Wasm table", error))?;
        }
        Ok(())
    }

    fn set_table_import(&mut self, table: &Table, index: u32, function: &Function) -> Result<()> {
        table
            .set(index, function)
            .map_err(|error| js_error("set Wasm table function", error))
    }
}
