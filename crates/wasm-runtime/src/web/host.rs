use super::Context;
use crate::{CallSignature, FunctionContext};
use js_sys::Function as JsFunction;
use relocade_wasm_bindings::{Flat, Value, WasmTuple};

impl<S: CallSignature + 'static> FunctionContext<S> for Context<'_>
where
    Flat<S::Parameters>: WasmTuple,
    Flat<S::Return>: WasmTuple,
{
    fn wrap_function(
        &mut self,
        callback: impl Fn(Context<'_>, Value<S::Parameters>) -> Value<S::Return> + 'static,
    ) -> JsFunction {
        relocade_wasm_bindings::callback::<S::Parameters, S::Return>(move |arguments| {
            callback(Context::new(), arguments)
        })
        .expect("create Rust callback adapter")
    }
}
