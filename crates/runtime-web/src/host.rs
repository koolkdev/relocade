use super::{Memory, Store, WebBackend};
use js_sys::Function as JsFunction;
use relocade_compiler::{Results, ValueType};
use relocade_runtime::wasm::{HostFunction, HostValue, WasmContext, WasmMemoryAccess};
use std::{cell::RefMut, rc::Rc};
use wasm_bindgen::{
    closure::Closure,
    convert::{FromWasmAbi, ReturnWasmAbi},
    JsCast,
};

macro_rules! host_function {
    ($($parameter:ident : $argument:ident),*) => {
        impl<T, R, $($parameter),*> HostFunction<fn($($parameter),*) -> R> for WebBackend<T>
        where
            T: 'static,
            R: Results + HostValue,
            R::Carrier: ReturnWasmAbi,
            $($parameter: ValueType + HostValue, $parameter::Carrier: FromWasmAbi),*
        {
            fn host_function(
                &mut self,
                callback: Box<dyn Fn(Context<'_, T>, $($parameter::Carrier),*) -> R::Carrier + Send + Sync>,
            ) -> JsFunction {
                let store = Rc::clone(&self.store);
                let closure = Closure::<dyn Fn($($parameter::Carrier),*) -> R::Carrier>::new(move |$($argument),*| {
                    callback(Context(store.borrow_mut()), $($argument),*)
                });
                let function = closure.as_ref().unchecked_ref::<JsFunction>().clone();
                self.callbacks.push(Box::new(closure));
                function
            }
        }
    };
}

host_function!();
host_function!(A: a);
host_function!(A: a, B: b);
host_function!(A: a, B: b, C: c);
host_function!(A: a, B: b, C: c, D: d);

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

impl<T> WasmMemoryAccess for Context<'_, T> {
    type Memory = Memory;

    fn read_memory(&self, memory: &Self::Memory, offset: u32, bytes: &mut [u8]) {
        self.0.read_memory(memory, offset, bytes);
    }
    fn write_memory(&mut self, memory: &Self::Memory, offset: u32, bytes: &[u8]) {
        self.0.write_memory(memory, offset, bytes);
    }
}
