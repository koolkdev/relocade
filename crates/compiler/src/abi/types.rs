use crate::{Results, Val, ValueType};

use relocade_wasm_bindings::AbiType;

/// The compiler representation of a Rust ABI type.
/// Scalars become typed `Val`s; derived aggregates retain their named fields.
/// The portable ABI owns the logical shape; this trait only projects IR values.
pub trait CompilerValue: AbiType<Logical: Results> {
    type Values: Clone + Into<crate::Arguments>;
    fn from_values(values: <Self::Logical as Results>::Values) -> Self::Values;
    fn into_values(values: Self::Values) -> <Self::Logical as Results>::Values;
}

/// The symbolic values corresponding to a Rust ABI type.
pub type Values<T> = <T as CompilerValue>::Values;

impl<T: ValueType> CompilerValue for T {
    type Values = Val<T>;

    fn into_values(value: Val<T>) -> Val<T> {
        value
    }

    fn from_values(value: Val<T>) -> Val<T> {
        value
    }
}

macro_rules! scalar {
    ($rust:ty) => {
        impl CompilerValue for $rust {
            type Values = Val<<Self as AbiType>::Logical>;

            fn into_values(value: Self::Values) -> Self::Values {
                value
            }

            fn from_values(value: Self::Values) -> Self::Values {
                value
            }
        }
    };
}

scalar!(bool);
scalar!(u8);
scalar!(i8);
scalar!(u16);
scalar!(i16);
scalar!(u32);
scalar!(i32);
scalar!(u64);
scalar!(i64);
scalar!(f64);

impl CompilerValue for () {
    type Values = ();

    fn from_values(_: ()) {}
    fn into_values(_: ()) {}
}

impl<T: CompilerValue, const N: usize> CompilerValue for [T; N] {
    type Values = [T::Values; N];

    fn from_values(values: <Self::Logical as Results>::Values) -> Self::Values {
        values.map(T::from_values)
    }
    fn into_values(values: Self::Values) -> <Self::Logical as Results>::Values {
        values.map(T::into_values)
    }
}

macro_rules! tuple {
    () => {};
    (@rest $first:ident $value:ident $(, $rest:ident $rest_value:ident)*) => {
        tuple!($($rest $rest_value),*);
    };
    ($($element:ident $value:ident),+) => {
        impl<$($element: CompilerValue),+> CompilerValue for ($($element,)+) {
            type Values = ($($element::Values,)+);

            fn from_values(values: <Self::Logical as Results>::Values) -> Self::Values {
                let ($($value,)+) = values;
                ($($element::from_values($value),)+)
            }
            fn into_values(values: Self::Values) -> <Self::Logical as Results>::Values {
                let ($($value,)+) = values;
                ($($element::into_values($value),)+)
            }
        }
        tuple!(@rest $($element $value),+);
    };
}

tuple!(A a, B b, C c, D d, E e, F f, G g, H h, I i, J j, K k, L l);
