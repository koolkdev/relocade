use super::{Backend, Memory};
use crate::{HostFunction, HostSignature, MemoryAccess, WasmContext, WasmMemoryAccess};
use relocade_wasm_bindings::{lift_flat, lower_flat, Flat, Value};
use wasmtime::{AsContextMut, Caller, Func, Store, StoreContextMut, WasmRet, WasmTy};

impl<T: 'static, S: HostSignature<Self> + 'static> HostFunction<S> for Backend<T>
where
    S::Callback: Send + Sync + 'static,
    Flat<S::Parameters>: Parameters,
    Flat<S::Return>: WasmRet,
{
    fn host_function(&mut self, callback: S::Callback) -> Func {
        <Flat<S::Parameters> as Parameters>::wrap(&mut self.store, move |mut caller, arguments| {
            let arguments: Value<S::Parameters> =
                lift_flat::<S::Parameters>(arguments).expect("invalid Wasm ABI value");
            lower_flat::<S::Return>(S::invoke(
                &callback,
                Context(caller.as_context_mut()),
                arguments,
            ))
        })
    }
}

trait Parameters: Sized + 'static {
    fn wrap<T: 'static, R: WasmRet>(
        store: &mut Store<T>,
        invoke: impl Fn(Caller<'_, T>, Self) -> R + Send + Sync + 'static,
    ) -> Func;
}

macro_rules! parameters {
    (@rest) => {};
    (@rest $first:ident $value:ident $(, $rest:ident $rest_value:ident)*) => {
        parameters!($($rest $rest_value),*);
    };
    ($($parameter:ident $argument:ident),*) => {
        impl<$($parameter: WasmTy + 'static),*> Parameters for ($($parameter,)*) {
            fn wrap<T: 'static, R: WasmRet>(
                store: &mut Store<T>,
                invoke: impl Fn(Caller<'_, T>, Self) -> R + Send + Sync + 'static,
            ) -> Func {
                Func::wrap(store, move |caller: Caller<'_, T>, $($argument: $parameter),*| invoke(caller, ($($argument,)*)))
            }
        }
        parameters!(@rest $($parameter $argument),*);
    };
}
parameters!(A a, B b, C c, D d, E e, F f, G g, H h, I i, J j, K k, L l);

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

    fn memory_and_state<'a>(&'a mut self, memory: &'a Memory) -> (MemoryAccess<'a>, &'a mut T) {
        memory.access_and_state(&mut self.0)
    }
}

impl<T: 'static> WasmMemoryAccess for Context<'_, T> {
    type Memory = Memory;

    fn memory<'a>(&'a mut self, memory: &'a Memory) -> MemoryAccess<'a> {
        memory.access(self.0.as_context_mut())
    }
}
