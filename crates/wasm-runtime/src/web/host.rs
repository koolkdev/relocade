use super::{Backend, Memory, Store};
use crate::{HostFunction, HostSignature, WasmContext, WasmMemoryAccess};
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

impl<T> WasmMemoryAccess for Context<'_, T> {
    type Memory = Memory;

    fn read_memory(&self, memory: &Self::Memory, offset: u32, bytes: &mut [u8]) {
        self.0.read_memory(memory, offset, bytes);
    }
    fn write_memory(&mut self, memory: &Self::Memory, offset: u32, bytes: &[u8]) {
        self.0.write_memory(memory, offset, bytes);
    }
}
