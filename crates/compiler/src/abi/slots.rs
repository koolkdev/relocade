//! Compiler values for the portable ABI's scalar layouts.
use super::{CompilerValue, WasmValues};
use crate::{Results, Val, ValueType, F64, I64};
pub use portable::{Cons, Layout, Tuple};
use relocade_wasm_bindings::{slots as portable, union::Cast};

/// The compiler's representation of a scalar slot.
pub struct Ir;

pub trait Scalar: portable::Scalar + ValueType {
    fn constant(value: Self::Carrier) -> Val<Self>;
}
impl<S: Scalar> portable::Representation<S> for Ir {
    type Value = Val<S>;
    fn constant(value: S::Carrier) -> Val<S> {
        S::constant(value)
    }
}

impl<T: Scalar> Cast<T, T> for Ir {
    fn cast(value: Val<T>) -> Val<T> {
        value
    }
}

macro_rules! integers {
    ($small:ty, $large:ty, $small_carrier:ty, $large_carrier:ty) => {
        impl Cast<$small, $large> for Ir {
            fn cast(value: Val<$small>) -> Val<$large> {
                value.unsigned().extend()
            }
        }
        impl Cast<$large, $small> for Ir {
            fn cast(value: Val<$large>) -> Val<$small> {
                value.truncate()
            }
        }
    };
}
relocade_wasm_bindings::integer_pairs!(integers);

impl Cast<F64, I64> for Ir {
    fn cast(value: Val<F64>) -> Val<I64> {
        value.to_bits()
    }
}
impl Cast<I64, F64> for Ir {
    fn cast(value: Val<I64>) -> Val<F64> {
        Val::<F64>::from_bits(value)
    }
}

pub trait Slots: portable::Slots + portable::Slots<Ir, Carrier = Self::Values> + Results {}
impl<S: portable::Slots + portable::Slots<Ir, Carrier = Self::Values> + Results> Slots for S {}

impl<H: Scalar, T: Slots> WasmValues for Cons<H, T> {
    fn flatten_values(values: Self::Values) -> Self::Values {
        values
    }
    fn unflatten_values(values: Self::Values) -> Self::Values {
        values
    }
}

pub fn value_slots<T: CompilerValue<Logical: WasmValues>>(
    value: T::Values,
) -> <Layout<T> as Results>::Values {
    T::Logical::flatten_values(T::into_values(value))
}
pub fn from_value_slots<T: CompilerValue<Logical: WasmValues>>(
    value: <Layout<T> as Results>::Values,
) -> T::Values {
    T::from_values(T::Logical::unflatten_values(value))
}
