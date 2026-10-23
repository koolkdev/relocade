use super::{Backend, Store};
use crate::{HostFunction, HostSignature, WasmContext};
use js_sys::Function as JsFunction;
use relocade_wasm_bindings::{Flat, WasmTuple};
use std::{cell::RefMut, rc::Rc};

impl<T: 'static, S: HostSignature<Self> + 'static> HostFunction<S> for Backend<T>
where
    S::Callback: 'static,
    Flat<S::Parameters>: WasmTuple,
    Flat<S::Return>: WasmTuple,
{
    fn host_function(&mut self, callback: S::Callback) -> JsFunction {
        let store = Rc::clone(&self.store);
        relocade_wasm_bindings::callback::<S::Parameters, S::Return>(move |arguments| {
            S::invoke(&callback, Context(store.borrow_mut()), arguments)
        })
        .expect("create Rust callback adapter")
    }
}

/// Scoped access to host state and memories, inside or outside a callback.
pub struct Context<'a, T>(pub(super) RefMut<'a, Store<T>>);

impl<T> WasmContext for Context<'_, T> {
    type State = T;

    fn state(&self) -> &T {
        &self.0.state
    }
    fn state_mut(&mut self) -> &mut T {
        &mut self.0.state
    }
}
