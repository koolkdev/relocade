use super::{Backend, Context};
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
        // Engine functions can outlive the backend without retaining its state.
        let store = Rc::downgrade(&self.store);
        relocade_wasm_bindings::callback::<S::Parameters, S::Return>(move |arguments| {
            let store = store.upgrade().expect("callback requires its live backend");
            S::invoke(&callback, Context(store.borrow_mut()), arguments)
        })
        .expect("create Rust callback adapter")
    }
}
