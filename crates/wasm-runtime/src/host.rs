use super::{CallSignature, WasmBackend};
use relocade_wasm_bindings::{slots::Tuple, AbiValue, Value, WasmRepr};

/// Derives a scoped Rust callback from the shared Rust function signature.
/// Aggregate fields are flattened by the shared ABI before engine adaptation.
pub trait HostSignature<B: WasmBackend>: CallSignature {
    type Callback: Send + Sync;
    fn invoke<'a>(
        callback: &'a Self::Callback,
        context: B::Context<'a>,
        arguments: Value<Self::Parameters>,
    ) -> Value<Self::Return>
    where
        B: 'a;
}

macro_rules! signature {
    (@rest) => {};
    (@rest $first:ident $value:ident $(, $rest:ident $rest_value:ident)*) => {
        signature!($($rest $rest_value),*);
    };
    ($($parameter:ident $argument:ident),*) => {
        impl<Backend, R, $($parameter),*> HostSignature<Backend> for fn($($parameter),*) -> R
        where
            Backend: WasmBackend,
            R: AbiValue<Logical: WasmRepr<Slots: Tuple>>,
            $($parameter: AbiValue,)*
            ($($parameter,)*): AbiValue<Value = ($(Value<$parameter>,)*), Logical: WasmRepr<Slots: Tuple>>,
        {
            type Callback = Box<dyn Fn(Backend::Context<'_>, $(Value<$parameter>),*) -> Value<R> + Send + Sync>;
            fn invoke<'a>(callback: &'a Self::Callback, context: Backend::Context<'a>, arguments: Value<Self::Parameters>) -> Value<R> where Backend: 'a {
                let ($($argument,)*) = arguments;
                callback(context, $($argument),*)
            }
        }
        signature!(@rest $($parameter $argument),*);
    };
}

signature!(A a, B b, C c, D d, E e, F f, G g, H h);

/// Backend support for wrapping one logical signature as a typed host function.
/// Implementations adapt argument arities, rather than enumerate signature shapes.
pub trait HostFunction<S: HostSignature<Self>>: WasmBackend {
    fn host_function(&mut self, callback: S::Callback) -> Self::ImportFunction;
}
