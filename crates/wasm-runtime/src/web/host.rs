use super::{Backend, Context};
use crate::{CallSignature, FunctionContext};
use js_sys::Function as JsFunction;
use relocade_wasm_bindings::{Flat, Value, WasmTuple};
use std::rc::Rc;

impl<T: 'static, S: CallSignature + 'static> FunctionContext<S> for Context<'_, T>
where
    Flat<S::Parameters>: WasmTuple,
    Flat<S::Return>: WasmTuple,
{
    type Backend = Backend<T>;
    type CallbackContext<'a> = Context<'a, T>;

    fn wrap_function(
        &mut self,
        callback: impl Fn(Context<'_, T>, Value<S::Parameters>) -> Value<S::Return> + 'static,
    ) -> JsFunction {
        // Engine functions can outlive the backend without retaining its state.
        let owner = Rc::downgrade(self.owner);
        relocade_wasm_bindings::callback::<S::Parameters, S::Return>(move |arguments| {
            let owner = owner.upgrade().expect("callback requires its live backend");
            callback(
                Context {
                    store: owner.borrow_mut(),
                    owner: &owner,
                },
                arguments,
            )
        })
        .expect("create Rust callback adapter")
    }
}
