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
    fn wrap_host_function(context: &mut Context<'_, T>, callback: S::Callback) -> JsFunction {
        // Engine functions can outlive the backend without retaining its state.
        let store = Rc::downgrade(context.owner);
        relocade_wasm_bindings::callback::<S::Parameters, S::Return>(move |arguments| {
            let store = store.upgrade().expect("callback requires its live backend");
            S::invoke(
                &callback,
                Context {
                    store: store.borrow_mut(),
                    owner: &store,
                },
                arguments,
            )
        })
        .expect("create Rust callback adapter")
    }
}
