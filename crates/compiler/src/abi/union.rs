//! Symbolic conversions between enum variants and their portable payload layout.
use super::slots::{Cons, Scalar, Slots};
use crate::{Results, Val, F64, I1, I16, I32, I64, I8};
use relocade_wasm_bindings::union as portable;

/// Converts directly between a variant and its shared payload layout.
/// Missing slots are zero-filled; slots beyond the destination are discarded.
pub trait Project<R: Slots>: Slots + portable::Project<R> {
    fn project_values(value: Self::Values) -> R::Values;
}

impl<R: Slots> Project<R> for () {
    fn project_values(_: ()) -> R::Values {
        R::zero_values()
    }
}

impl<H: Scalar, T: Slots> Project<()> for Cons<H, T> {
    fn project_values(_: Self::Values) {}
}

impl<A: Scalar + Cast<B>, B: Scalar, T: Slots + Project<U>, U: Slots> Project<Cons<B, U>>
    for Cons<A, T>
{
    fn project_values(value: Self::Values) -> <Cons<B, U> as Results>::Values {
        (A::cast_value(value.0), T::project_values(value.1))
    }
}

pub trait Cast<R: Scalar>: Scalar + portable::Cast<R> {
    fn cast_value(value: Val<Self>) -> Val<R>;
}

impl<T: Scalar> Cast<T> for T {
    fn cast_value(value: Val<Self>) -> Val<Self> {
        value
    }
}

macro_rules! integers {
    ($small:ty, $large:ty) => {
        impl Cast<$large> for $small {
            fn cast_value(value: Val<Self>) -> Val<$large> {
                value.unsigned().extend()
            }
        }
        impl Cast<$small> for $large {
            fn cast_value(value: Val<Self>) -> Val<$small> {
                value.truncate()
            }
        }
    };
}
integers!(I1, I8);
integers!(I1, I16);
integers!(I1, I32);
integers!(I1, I64);
integers!(I8, I16);
integers!(I8, I32);
integers!(I8, I64);
integers!(I16, I32);
integers!(I16, I64);
integers!(I32, I64);

impl Cast<I64> for F64 {
    fn cast_value(value: Val<Self>) -> Val<I64> {
        value.to_bits()
    }
}
impl Cast<F64> for I64 {
    fn cast_value(value: Val<Self>) -> Val<F64> {
        Val::<F64>::from_bits(value)
    }
}
