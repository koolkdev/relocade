use super::{Context, StoreData};
use crate::{CallSignature, FunctionContext};
use relocade_wasm_bindings::{lift_flat, lower_flat, Flat, Value};
use wasmtime::{AsContextMut, Caller, Func, StoreContextMut, WasmRet, WasmTy};

mod local;

impl<S: CallSignature + 'static> FunctionContext<S> for Context<'_>
where
    Flat<S::Parameters>: Parameters,
    Flat<S::Return>: WasmRet,
{
    fn wrap_function(
        &mut self,
        callback: impl Fn(Context<'_>, Value<S::Parameters>) -> Value<S::Return> + 'static,
    ) -> Func {
        wrap::<S>(self, move |caller, arguments| {
            callback(Context(caller.into()), arguments)
        })
    }
}

fn wrap<S: CallSignature + 'static>(
    context: &mut Context<'_>,
    invoke: impl Fn(Caller<'_, StoreData>, Value<S::Parameters>) -> Value<S::Return> + 'static,
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
    fn wrap<R: WasmRet>(
        store: StoreContextMut<'_, StoreData>,
        invoke: impl Fn(Caller<'_, StoreData>, Self) -> R + Send + Sync + 'static,
    ) -> Func;
}

macro_rules! parameters {
    (@rest) => {};
    (@rest $first:ident $value:ident $(, $rest:ident $rest_value:ident)*) => {
        parameters!($($rest $rest_value),*);
    };
    ($($parameter:ident $argument:ident),*) => {
        impl<$($parameter: WasmTy + 'static),*> Parameters for ($($parameter,)*) {
            fn wrap<R: WasmRet>(
                store: StoreContextMut<'_, StoreData>,
                invoke: impl Fn(Caller<'_, StoreData>, Self) -> R + Send + Sync + 'static,
            ) -> Func {
                Func::wrap(store, move |caller: Caller<'_, StoreData>, $($argument: $parameter),*| invoke(caller, ($($argument,)*)))
            }
        }
        parameters!(@rest $($parameter $argument),*);
    };
}
parameters!(A a, B b, C c, D d, E e, F f, G g, H h, I i, J j, K k, L l);
