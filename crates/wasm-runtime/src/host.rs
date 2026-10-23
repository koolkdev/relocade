use super::WasmBackend;
use relocade_compiler::{abi::FunctionSignature, Results, ValueType, F64, I1, I16, I32, I64, I8};

/// Rust carrier for a logical scalar or flat result tuple. Narrow integers use
/// Wasm's i32 carrier, matching the compiler's import ABI. Unit is a void result.
pub trait HostValue {
    type Carrier: 'static;
}

macro_rules! carrier {
    ($logical:ty, $rust:ty) => {
        impl HostValue for $logical {
            type Carrier = $rust;
        }
    };
}

carrier!((), ());
carrier!(I1, i32);
carrier!(I8, i32);
carrier!(I16, i32);
carrier!(I32, i32);
carrier!(I64, i64);
carrier!(F64, f64);

macro_rules! tuple_carrier {
    ($($element:ident),+) => {
        impl<$($element: ValueType + HostValue),+> HostValue for ($($element,)+) {
            type Carrier = ($($element::Carrier,)+);
        }
    };
}

tuple_carrier!(A);
tuple_carrier!(A, B);
tuple_carrier!(A, B, C);
tuple_carrier!(A, B, C, D);
tuple_carrier!(A, B, C, D, E);
tuple_carrier!(A, B, C, D, E, F);
tuple_carrier!(A, B, C, D, E, F, G);
tuple_carrier!(A, B, C, D, E, F, G, H);
tuple_carrier!(A, B, C, D, E, F, G, H, I);
tuple_carrier!(A, B, C, D, E, F, G, H, I, J);
tuple_carrier!(A, B, C, D, E, F, G, H, I, J, K);
tuple_carrier!(A, B, C, D, E, F, G, H, I, J, K, L);

/// Derives a scoped Rust callback from the shared logical function signature.
/// Host callbacks support scalar parameters and scalar or flat tuple results.
pub trait HostSignature<B: WasmBackend>: FunctionSignature {
    type Callback;
}

macro_rules! signature {
    ($($parameter:ident),*) => {
        impl<Backend, R, $($parameter),*> HostSignature<Backend> for fn($($parameter),*) -> R
        where
            Backend: WasmBackend,
            R: Results + HostValue + relocade_compiler::abi::AbiType<Logical = R>,
            $($parameter: ValueType + HostValue),*
        {
            type Callback = Box<dyn Fn(Backend::Context<'_>, $($parameter::Carrier),*) -> R::Carrier + Send + Sync>;
        }
    };
}

signature!();
signature!(A);
signature!(A, B);
signature!(A, B, C);
signature!(A, B, C, D);

/// Backend support for wrapping one logical signature as a typed host function.
/// Implementations adapt argument arities, rather than enumerate signature shapes.
pub trait HostFunction<S: HostSignature<Self>>: WasmBackend {
    fn host_function(&mut self, callback: S::Callback) -> Self::ImportFunction;
}
