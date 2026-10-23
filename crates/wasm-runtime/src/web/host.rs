use super::Backend;
use crate::{HostFunction, HostSignature};
use js_sys::Function as JsFunction;
use relocade_wasm_bindings::{Flat, WasmTuple};
use std::rc::Rc;

impl<T: 'static, S: HostSignature<Self> + 'static> HostFunction<S> for Backend<T>
where
    S::Callback: 'static,
    Flat<S::Parameters>: WasmTuple,
    Flat<S::Return>: WasmTuple,
{
    fn host_function(&mut self, callback: S::Callback) -> JsFunction {
        let store = Rc::clone(&self.store);
        relocade_wasm_bindings::callback::<S::Parameters, S::Return>(move |arguments| {
            S::invoke(&callback, store.callback(), arguments)
        })
        .expect("create Rust callback adapter")
    }
}
