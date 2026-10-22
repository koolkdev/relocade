use super::{Backend, Memory, SharedMemory, Store};
use crate::{HostFunction, HostValue, MemoryBytes, WasmContext, WasmMemoryAccess};
use js_sys::{Array, Function as JsFunction};
use relocade_compiler::{Results, ValueType};
use std::{cell::RefMut, rc::Rc};
use wasm_bindgen::{
    closure::Closure,
    convert::{FromWasmAbi, ReturnWasmAbi},
    JsCast, JsValue,
};

// JavaScript imports return an iterable for Wasm multi-value results. Keep that
// conversion at the web boundary; native callbacks retain ordinary Rust tuples.
trait HostResult: 'static {
    type Return: ReturnWasmAbi;
    fn into_return(self) -> Self::Return;
}

macro_rules! scalar_result {
    ($($scalar:ty),+) => { $(
        impl HostResult for $scalar {
            type Return = Self;
            fn into_return(self) -> Self { self }
        }
    )+ };
}
scalar_result!((), i32, i64, f64);

impl<A: HostResult> HostResult for (A,) {
    type Return = A::Return;
    fn into_return(self) -> Self::Return {
        self.0.into_return()
    }
}

macro_rules! tuple_result {
    ($($element:ident $index:tt),+) => {
        impl<$($element: Into<JsValue> + 'static),+> HostResult for ($($element,)+) {
            type Return = JsValue;
            fn into_return(self) -> JsValue {
                let values = Array::new();
                $(values.push(&self.$index.into());)+
                values.into()
            }
        }
    };
}

tuple_result!(A 0, B 1);
tuple_result!(A 0, B 1, C 2);
tuple_result!(A 0, B 1, C 2, D 3);
tuple_result!(A 0, B 1, C 2, D 3, E 4);
tuple_result!(A 0, B 1, C 2, D 3, E 4, F 5);
tuple_result!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
tuple_result!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
tuple_result!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);
tuple_result!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9);
tuple_result!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10);
tuple_result!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11);

macro_rules! host_function {
    ($($parameter:ident : $argument:ident),*) => {
        impl<T, R, $($parameter),*> HostFunction<fn($($parameter),*) -> R> for Backend<T>
        where
            T: 'static,
            R: Results + HostValue,
            R::Carrier: HostResult,
            $($parameter: ValueType + HostValue, $parameter::Carrier: FromWasmAbi),*
        {
            fn host_function(
                &mut self,
                callback: Box<dyn Fn(Context<'_, T>, $($parameter::Carrier),*) -> R::Carrier + Send + Sync>,
            ) -> JsFunction {
                let store = Rc::clone(&self.store);
                let closure = Closure::<dyn Fn($($parameter::Carrier),*) -> <R::Carrier as HostResult>::Return>::new(move |$($argument),*| {
                    callback(Context(store.borrow_mut()), $($argument),*).into_return()
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
    type SharedMemory = SharedMemory;

    fn memory_bytes<'a>(&'a mut self, memory: &'a Memory) -> MemoryBytes<'a> {
        self.0.memory_bytes(memory)
    }

    fn shared_memory(&self, memory: &Memory) -> Option<SharedMemory> {
        self.0.shared_memory(memory)
    }

    fn read_memory(&self, memory: &Self::Memory, offset: u32, bytes: &mut [u8]) {
        self.0.read_memory(memory, offset, bytes);
    }
    fn write_memory(&mut self, memory: &Self::Memory, offset: u32, bytes: &[u8]) {
        self.0.write_memory(memory, offset, bytes);
    }
}
