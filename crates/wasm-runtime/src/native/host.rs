use super::{Backend, Context, StoreData};
use crate::{CallSignature, FunctionContext};
use relocade_wasm_bindings::{lift_flat, lower_flat, Flat, Value};
use wasmtime::{AsContextMut, Caller, Func, StoreContextMut, WasmRet, WasmTy};

mod local;

impl<T: 'static, S: CallSignature + 'static> FunctionContext<S> for Context<'_, T>
where
    Flat<S::Parameters>: Parameters,
    Flat<S::Return>: WasmRet,
{
    type Backend = Backend<T>;
    type CallbackContext<'a> = Context<'a, T>;

    fn wrap_function(
        &mut self,
        callback: impl Fn(Context<'_, T>, Value<S::Parameters>) -> Value<S::Return> + 'static,
    ) -> Func {
        wrap::<T, S>(self, move |caller, arguments| {
            callback(Context(caller.into()), arguments)
        })
    }
}

fn wrap<T: 'static, S: CallSignature + 'static>(
    context: &mut Context<'_, T>,
    invoke: impl Fn(Caller<'_, StoreData<T>>, Value<S::Parameters>) -> Value<S::Return> + 'static,
) -> Func
where
    Flat<S::Parameters>: Parameters,
    Flat<S::Return>: WasmRet,
{
    // Only this private, thread-local store can invoke or destroy the closure.
    let invoke = local::Callback::new(invoke);
    <Flat<S::Parameters> as Parameters>::wrap(
        context.0.as_context_mut(),
        move |caller, arguments| {
            let arguments = lift_flat::<S::Parameters>(arguments).expect("invalid Wasm ABI value");
            lower_flat::<S::Return>(invoke.get()(caller, arguments))
        },
    )
}

trait Parameters: Sized + 'static {
    fn wrap<T: 'static, R: WasmRet>(
        store: StoreContextMut<'_, StoreData<T>>,
        invoke: impl Fn(Caller<'_, StoreData<T>>, Self) -> R + Send + Sync + 'static,
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
                store: StoreContextMut<'_, StoreData<T>>,
                invoke: impl Fn(Caller<'_, StoreData<T>>, Self) -> R + Send + Sync + 'static,
            ) -> Func {
                Func::wrap(store, move |caller: Caller<'_, StoreData<T>>, $($argument: $parameter),*| invoke(caller, ($($argument,)*)))
            }
        }
        parameters!(@rest $($parameter $argument),*);
    };
}
parameters!(A a, B b, C c, D d, E e, F f, G g, H h, I i, J j, K k, L l);
