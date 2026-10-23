use super::Backend;
use crate::{CallSignature, Error, ExportFunction, Result};
use relocade_compiler::abi::{Function, Value};
use relocade_wasm_bindings::{lift_flat, lower_flat, Flat};
use wasmtime::{Func, Instance, TypedFunc, WasmParams, WasmResults};

impl<T: 'static, S: CallSignature> ExportFunction<S> for Backend<T>
where
    Flat<S::Parameters>: WasmParams,
    Flat<S::Return>: WasmResults,
{
    type Handle = TypedFunc<Flat<S::Parameters>, Flat<S::Return>>;

    fn resolve_function(
        &mut self,
        instance: &Instance,
        declaration: Function<S>,
    ) -> Result<Self::Handle> {
        instance
            .get_typed_func(&mut self.store, declaration.name)
            .map_err(|error| Error::new("resolve Wasm function", error))
    }

    fn invoke_function(
        &mut self,
        function: &Self::Handle,
        arguments: Value<S::Parameters>,
    ) -> Value<S::Return> {
        let result = function
            .call(&mut self.store, lower_flat::<S::Parameters>(arguments))
            .expect("Wasm execution trapped");
        lift_flat::<S::Return>(result).expect("invalid Wasm ABI value")
    }

    fn function_import(&self, function: &Self::Handle) -> Func {
        *function.func()
    }
}
