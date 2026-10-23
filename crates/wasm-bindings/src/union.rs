//! Overlapping enum payloads use the widest logical slot at each position.

use super::slots::{Cons, Native, Representation, Scalar, Slots, Values};
use crate::{F32, F64, I1, I16, I32, I64, I8};

/// The integer width pairs shared by native and compiler scalar conversions.
#[doc(hidden)]
#[macro_export]
macro_rules! integer_pairs {
    ($convert:ident) => {
        $convert!($crate::I1, $crate::I8, i32, i32);
        $convert!($crate::I1, $crate::I16, i32, i32);
        $convert!($crate::I1, $crate::I32, i32, i32);
        $convert!($crate::I1, $crate::I64, i32, i64);
        $convert!($crate::I8, $crate::I16, i32, i32);
        $convert!($crate::I8, $crate::I32, i32, i32);
        $convert!($crate::I8, $crate::I64, i32, i64);
        $convert!($crate::I16, $crate::I32, i32, i32);
        $convert!($crate::I16, $crate::I64, i32, i64);
        $convert!($crate::I32, $crate::I64, i32, i64);
    };
}

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
pub trait Project<R: Slots<V>, V = Native>: Slots<V> {
    fn project(value: Values<Self, V>) -> Values<R, V>;
}

impl<R: Slots<V>, V> Project<R, V> for () {
    fn project(_: ()) -> Values<R, V> {
        <R as Slots<V>>::zero()
    }
}

impl<H: Scalar, T: Slots + Slots<V>, V: Representation<H>> Project<(), V> for Cons<H, T> {
    fn project(_: Values<Self, V>) {}
}

impl<A: Scalar, B: Scalar, T: Slots + Project<U, V>, U: Slots + Slots<V>, V: Cast<A, B>>
    Project<Cons<B, U>, V> for Cons<A, T>
{
    fn project(value: Values<Self, V>) -> Values<Cons<B, U>, V> {
        (V::cast(value.0), T::project(value.1))
    }
}

impl<T: Scalar> Widen<T> for T {
    type Output = T;
}

pub trait Cast<A: Scalar, B: Scalar>: Representation<A> + Representation<B> {
    fn cast(value: <Self as Representation<A>>::Value) -> <Self as Representation<B>>::Value;
}

impl<T: Scalar> Cast<T, T> for Native {
    #[inline]
    fn cast(value: T::Carrier) -> T::Carrier {
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
        impl Cast<$small, $large> for Native {
            #[inline]
            fn cast(value: $small_carrier) -> $large_carrier {
                value as u32 as $large_carrier
            }
        }
        impl Cast<$large, $small> for Native {
            #[inline]
            fn cast(value: $large_carrier) -> $small_carrier {
                value as $small_carrier
            }
        }
    };
}
integer_pairs!(integers);

macro_rules! floating {
    ($($integer:ty),+) => { $(
        impl Widen<$integer> for F64 { type Output = I64; }
        impl Widen<F64> for $integer { type Output = I64; }
    )+ };
}
floating!(I1, I8, I16, I32, I64);

impl Cast<F64, I64> for Native {
    #[inline]
    fn cast(value: f64) -> i64 {
        value.to_bits() as i64
    }
}
impl Cast<I64, F64> for Native {
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
        impl Cast<F32, $integer> for Native {
            fn cast(value: f32) -> $carrier {
                value.to_bits() as $carrier
            }
        }
        impl Cast<$integer, F32> for Native {
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
