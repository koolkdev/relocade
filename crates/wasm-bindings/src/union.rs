//! Overlapping enum payloads use the widest logical slot at each position.

use super::slots::{Cons, Scalar, Slots};
use crate::{F32, F64, I1, I16, I32, I64, I8};

/// Computes the shared payload layout, preserving the widest slot at each position.
pub trait Join<R: Slots>: Slots {
    type Output: Slots;
}

pub trait Widen<R: Scalar>: Scalar {
    type Output: Scalar;
}

impl<R: Slots> Join<R> for () {
    type Output = R;
}

impl<H: Scalar, T: Slots> Join<()> for Cons<H, T> {
    type Output = Self;
}

impl<A: Scalar + Widen<B>, B: Scalar, T: Slots + Join<U>, U: Slots> Join<Cons<B, U>>
    for Cons<A, T>
{
    type Output = Cons<<A as Widen<B>>::Output, T::Output>;
}

/// Converts directly between a variant and its shared payload layout.
/// Missing slots are zero-filled; slots beyond the destination are discarded.
pub trait Project<R: Slots>: Slots {
    fn project(value: Self::Carrier) -> R::Carrier;
}

impl<R: Slots> Project<R> for () {
    fn project(_: ()) -> R::Carrier {
        R::zero()
    }
}

impl<H: Scalar, T: Slots> Project<()> for Cons<H, T> {
    fn project(_: Self::Carrier) {}
}

impl<A: Scalar + Cast<B>, B: Scalar, T: Slots + Project<U>, U: Slots> Project<Cons<B, U>>
    for Cons<A, T>
{
    fn project(value: Self::Carrier) -> <Cons<B, U> as Slots>::Carrier {
        (A::cast(value.0), T::project(value.1))
    }
}

impl<T: Scalar> Widen<T> for T {
    type Output = T;
}

pub trait Cast<R: Scalar>: Scalar {
    fn cast(value: Self::Carrier) -> R::Carrier;
}

impl<T: Scalar> Cast<T> for T {
    #[inline]
    fn cast(value: Self::Carrier) -> Self::Carrier {
        value
    }
}

macro_rules! integers {
    ($small:ty, $large:ty, $small_carrier:ty, $large_carrier:ty) => {
        impl Widen<$large> for $small {
            type Output = $large;
        }
        impl Widen<$small> for $large {
            type Output = $large;
        }
        impl Cast<$large> for $small {
            #[inline]
            fn cast(value: $small_carrier) -> $large_carrier {
                value as u32 as $large_carrier
            }
        }
        impl Cast<$small> for $large {
            #[inline]
            fn cast(value: $large_carrier) -> $small_carrier {
                value as $small_carrier
            }
        }
    };
}
integers!(I1, I8, i32, i32);
integers!(I1, I16, i32, i32);
integers!(I1, I32, i32, i32);
integers!(I1, I64, i32, i64);
integers!(I8, I16, i32, i32);
integers!(I8, I32, i32, i32);
integers!(I8, I64, i32, i64);
integers!(I16, I32, i32, i32);
integers!(I16, I64, i32, i64);
integers!(I32, I64, i32, i64);

macro_rules! floating {
    ($($integer:ty),+) => { $(
        impl Widen<$integer> for F64 { type Output = I64; }
        impl Widen<F64> for $integer { type Output = I64; }
    )+ };
}
floating!(I1, I8, I16, I32, I64);

impl Cast<I64> for F64 {
    #[inline]
    fn cast(value: f64) -> i64 {
        value.to_bits() as i64
    }
}
impl Cast<F64> for I64 {
    #[inline]
    fn cast(value: i64) -> f64 {
        f64::from_bits(value as u64)
    }
}

macro_rules! f32_integer {
    ($integer:ty, $shared:ty, $carrier:ty) => {
        impl Widen<$integer> for F32 {
            type Output = $shared;
        }
        impl Widen<F32> for $integer {
            type Output = $shared;
        }
        impl Cast<$integer> for F32 {
            fn cast(value: f32) -> $carrier {
                value.to_bits() as $carrier
            }
        }
        impl Cast<F32> for $integer {
            fn cast(value: $carrier) -> f32 {
                f32::from_bits(value as u32)
            }
        }
    };
}
f32_integer!(I32, I32, i32);
f32_integer!(I64, I64, i64);
macro_rules! f32_narrow {
    ($($integer:ty),+) => { $(
        impl Widen<$integer> for F32 { type Output = I32; }
        impl Widen<F32> for $integer { type Output = I32; }
    )+ };
}
f32_narrow!(I1, I8, I16);
impl Widen<F64> for F32 {
    type Output = I64;
}
impl Widen<F32> for F64 {
    type Output = I64;
}
