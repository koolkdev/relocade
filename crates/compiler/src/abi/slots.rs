//! Symbolic operations on the portable ABI's scalar layouts.
use super::{Symbolic, WasmValues};
use crate::{Arguments, Results, Val, ValueType};
pub use portable::{Cons, Layout, Tuple};
use relocade_wasm_bindings::slots as portable;

pub trait Scalar: portable::Scalar + ValueType {
    fn zero_value() -> Val<Self>;
    fn constant(value: Self::Carrier) -> Val<Self>;
}

pub trait Slots: portable::Slots + Results {
    fn zero_values() -> Self::Values;
    fn clone_values(values: &Self::Values) -> Self::Values;
    fn arguments(values: Self::Values) -> Arguments;
    fn constants(values: Self::Carrier) -> Self::Values;
}
impl Slots for () {
    fn zero_values() {}
    fn clone_values(_: &()) {}
    fn arguments(_: ()) -> Arguments {
        ().into()
    }
    fn constants(_: ()) {}
}
impl<H: Scalar, T: Slots> WasmValues for Cons<H, T> {
    fn flatten_values(values: Self::Values) -> Self::Values {
        values
    }
    fn unflatten_values(values: Self::Values) -> Self::Values {
        values
    }
}
impl<H: Scalar, T: Slots> Slots for Cons<H, T> {
    fn zero_values() -> Self::Values {
        (H::zero_value(), T::zero_values())
    }
    fn clone_values(values: &Self::Values) -> Self::Values {
        (values.0.clone(), T::clone_values(&values.1))
    }
    fn arguments(values: Self::Values) -> Arguments {
        (values.0, T::arguments(values.1)).into()
    }
    fn constants(values: Self::Carrier) -> Self::Values {
        (H::constant(values.0), T::constants(values.1))
    }
}

pub trait Concat<R: Slots>: Slots + portable::Concat<R, Output: Slots> {
    fn concat_values(left: Self::Values, right: R::Values) -> <Self::Output as Results>::Values;
    fn split_values(value: <Self::Output as Results>::Values) -> (Self::Values, R::Values);
}
impl<R: Slots> Concat<R> for () {
    fn concat_values(_: (), right: R::Values) -> R::Values {
        right
    }
    fn split_values(value: R::Values) -> ((), R::Values) {
        ((), value)
    }
}
impl<H: Scalar, T: Slots + Concat<R>, R: Slots> Concat<R> for Cons<H, T> {
    fn concat_values(left: Self::Values, right: R::Values) -> <Self::Output as Results>::Values {
        (left.0, T::concat_values(left.1, right))
    }
    fn split_values(value: <Self::Output as Results>::Values) -> (Self::Values, R::Values) {
        let (tail, right) = T::split_values(value.1);
        ((value.0, tail), right)
    }
}

pub fn value_slots<T: Symbolic<Logical: WasmValues>>(
    value: T::Values,
) -> <Layout<T> as Results>::Values {
    T::Logical::flatten_values(T::into_values(value))
}
pub fn from_value_slots<T: Symbolic<Logical: WasmValues>>(
    value: <Layout<T> as Results>::Values,
) -> T::Values {
    T::from_values(T::Logical::unflatten_values(value))
}
