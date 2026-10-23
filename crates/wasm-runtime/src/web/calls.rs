use super::Backend;
use crate::{CallSignature, Error, ExportFunction, Result};
use js_sys::{Function as JsFunction, WebAssembly};
use relocade_compiler::abi::{Function, Value};
use relocade_wasm_bindings::{Flat, Function as Binding, WasmTuple};

impl<T: 'static, S: CallSignature> ExportFunction<S> for Backend<T>
where
    Flat<S::Parameters>: WasmTuple,
    Flat<S::Return>: WasmTuple,
{
    type Handle = Binding<S::Parameters, S::Return>;

    fn resolve_function(
        &mut self,
        instance: &WebAssembly::Instance,
        declaration: Function<S>,
    ) -> Result<Self::Handle> {
        Binding::from_export(instance, declaration.name)
            .map_err(|error| Error::new("bind Wasm export", error))
    }

    fn invoke_function(
        execution: &mut Self::Execution<'_>,
        function: &Self::Handle,
        arguments: Value<S::Parameters>,
    ) -> Value<S::Return> {
        execution.call(|| function.call(arguments))
    }

    fn function_import(&self, function: &Self::Handle) -> JsFunction {
        function.as_function().clone()
    }
}
