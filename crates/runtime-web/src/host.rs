use super::{Memory, Store, WebBackend};
use js_sys::Function as JsFunction;
use std::rc::Rc;
use wasm86_compiler::{Results, ValueType};
use wasm86_runtime::wasm::{HostFunction, HostValue, WasmCaller, WasmMemoryAccess};
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
                callback: Box<dyn Fn(Caller<'_, T>, $($parameter::Carrier),*) -> R::Carrier + Send + Sync>,
            ) -> JsFunction {
                let store = Rc::clone(&self.store);
                let closure = Closure::<dyn Fn($($parameter::Carrier),*) -> R::Carrier>::new(move |$($argument),*| {
                    callback(Caller(&mut *store.borrow_mut()), $($argument),*)
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

/// A callback's scoped access to host state and the backend's memories.
pub struct Caller<'a, T>(&'a mut Store<T>);

impl<T> WasmCaller for Caller<'_, T> {
    type State = T;

    fn state(&self) -> &T {
        &self.0.state
    }
    fn state_mut(&mut self) -> &mut T {
        &mut self.0.state
    }
}

impl<T> WasmMemoryAccess for Caller<'_, T> {
    type Memory = Memory;

    fn read_memory(&self, memory: &Self::Memory, offset: u32, bytes: &mut [u8]) {
        self.0.read_memory(memory, offset, bytes);
    }
    fn write_memory(&mut self, memory: &Self::Memory, offset: u32, bytes: &[u8]) {
        self.0.write_memory(memory, offset, bytes);
    }
}
