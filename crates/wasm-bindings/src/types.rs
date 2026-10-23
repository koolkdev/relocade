/// A Rust type's logical shape at a Wasm boundary.
/// Logical integer widths remain distinct from their Wasm carriers. Defining a
/// shape does not require a host codec or a compiler representation.
pub trait AbiType: 'static {
    type Logical: 'static;
}

macro_rules! marker {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub struct $name;
        impl AbiType for $name {
            type Logical = Self;
        }
    };
}
marker!(I1, "A logical one-bit integer carried by Wasm i32.");
marker!(I8, "A logical 8-bit integer carried by Wasm i32.");
marker!(I16, "A logical 16-bit integer carried by Wasm i32.");
marker!(I32, "A logical 32-bit integer carried by Wasm i32.");
marker!(I64, "A logical 64-bit integer carried by Wasm i64.");
marker!(F32, "An IEEE binary32 value carried by Wasm f32.");
marker!(F64, "An IEEE binary64 value carried by Wasm f64.");

macro_rules! scalar {
    ($rust:ty, $logical:ty) => {
        impl AbiType for $rust {
            type Logical = $logical;
        }
    };
}
scalar!(bool, I1);
scalar!(u8, I8);
scalar!(i8, I8);
scalar!(u16, I16);
scalar!(i16, I16);
scalar!(u32, I32);
scalar!(i32, I32);
scalar!(u64, I64);
scalar!(i64, I64);
scalar!(f32, F32);
scalar!(f64, F64);
impl AbiType for () {
    type Logical = ();
}
impl<T: AbiType, const N: usize> AbiType for [T; N] {
    type Logical = [T::Logical; N];
}
macro_rules! tuple {
    () => {};
    ($first:ident $(, $rest:ident)*) => {
        impl<$first: AbiType, $($rest: AbiType),*> AbiType for ($first, $($rest,)*) {
            type Logical = ($first::Logical, $($rest::Logical,)*);
        }
        tuple!($($rest),*);
    };
}
tuple!(A, B, C, D, E, F, G, H, I, J, K, L);

/// A Rust function-pointer shape whose parameters and return implement [`AbiType`].
/// Tuples, arrays and named aggregates preserve their logical field types.
pub trait FunctionSignature {
    type Parameters: AbiType;
    type Return: AbiType;
}
macro_rules! signatures {
    (@rest) => {};
    (@rest $first:ident $(, $rest:ident)*) => { signatures!($($rest),*); };
    ($($parameter:ident),*) => {
        impl<R: AbiType, $($parameter: AbiType),*> FunctionSignature for fn($($parameter),*) -> R {
            type Parameters = ($($parameter,)*);
            type Return = R;
        }
        signatures!(@rest $($parameter),*);
    };
}
signatures!(A, B, C, D, E, F, G, H);
