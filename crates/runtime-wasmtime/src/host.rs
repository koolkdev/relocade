use super::WasmtimeBackend;
use relocade_compiler::{Results, ValueType};
use relocade_runtime::wasm::{HostFunction, HostValue, WasmContext, WasmMemoryAccess};
use wasmtime::{AsContextMut, Func, Memory, StoreContextMut, WasmRet, WasmTy};

macro_rules! host_function {
    ($($parameter:ident : $argument:ident),*) => {
        impl<T, R, $($parameter),*> HostFunction<fn($($parameter),*) -> R> for WasmtimeBackend<T>
        where
            T: 'static,
            R: Results + HostValue,
            R::Carrier: WasmRet,
            $($parameter: ValueType + HostValue, $parameter::Carrier: WasmTy),*
        {
            fn host_function(
                &mut self,
                callback: Box<dyn Fn(Context<'_, T>, $($parameter::Carrier),*) -> R::Carrier + Send + Sync>,
            ) -> Func {
                Func::wrap(&mut self.store, move |mut caller: wasmtime::Caller<'_, T>, $($argument: $parameter::Carrier),*| {
                    callback(Context(caller.as_context_mut()), $($argument),*)
                })
            }
        }
    };
}

host_function!();
host_function!(A: a);
host_function!(A: a, B: b);
host_function!(A: a, B: b, C: c);
host_function!(A: a, B: b, C: c, D: d);

/// Scoped access to the store and its memories, inside or outside a callback.
pub struct Context<'a, T: 'static>(pub(super) StoreContextMut<'a, T>);

impl<T: 'static> WasmContext for Context<'_, T> {
    type State = T;

    fn state(&self) -> &T {
        self.0.data()
    }

    fn state_mut(&mut self) -> &mut T {
        self.0.data_mut()
    }
}

impl<T: 'static> WasmMemoryAccess for Context<'_, T> {
    type Memory = Memory;

    fn read_memory(&self, memory: &Memory, offset: u32, bytes: &mut [u8]) {
        memory
            .read(&self.0, offset as usize, bytes)
            .expect("in-bounds Wasm memory range");
    }

    fn write_memory(&mut self, memory: &Memory, offset: u32, bytes: &[u8]) {
        memory
            .write(&mut self.0, offset as usize, bytes)
            .expect("in-bounds Wasm memory range");
    }
}
