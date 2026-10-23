use super::{
    slots::{Concat, Cons, Layout, Scalar, Slots, Tuple},
    AbiType,
};
use crate::{F32, F64, I1, I16, I32, I64, I8};

/// Maps a logical shape to its ordered scalar slots at a Wasm boundary.
/// Compiler-only shapes do not need to implement this trait.
pub trait WasmRepr {
    type Slots: Slots;
}

/// The nested scalar-slot carrier derived from a type's logical shape.
pub type Carrier<T> = <Layout<T> as Slots>::Carrier;

/// The Rust value supplied to or returned from a bound function.
pub type Value<T> = <T as AbiValue>::Value;

/// Converts an ABI type's Rust values directly to or from its scalar slots.
///
/// Domain types implement this alongside `AbiType`, without depending on an
/// execution engine. The carrier comes from `Logical`; a codec cannot select a
/// different physical signature. A failed lift means the carrier does not
/// represent a value of this type. `lower` must respect the logical shape's
/// canonical values, including zero-extension of narrow integer components.
pub trait AbiValue: AbiType<Logical: WasmRepr> + Sized {
    type Value: 'static;

    fn lower(value: Self::Value) -> Carrier<Self>;
    fn lift(value: Carrier<Self>) -> Option<Self::Value>;
}

impl WasmRepr for () {
    type Slots = Self;
}

impl AbiValue for () {
    type Value = ();
    #[inline]
    fn lower(_: ()) {}
    #[inline]
    fn lift(_: ()) -> Option<()> {
        Some(())
    }
}

macro_rules! scalar {
    ($logical:ty, $carrier:ty, $zero:expr) => {
        impl Scalar for $logical {
            type Carrier = $carrier;
            #[inline]
            fn zero() -> $carrier {
                $zero
            }
        }
        impl WasmRepr for $logical {
            type Slots = Cons<Self, ()>;
        }
        impl AbiValue for $logical {
            type Value = $carrier;
            #[inline]
            fn lower(value: $carrier) -> Carrier<Self> {
                (value, ())
            }
            #[inline]
            fn lift(value: Carrier<Self>) -> Option<$carrier> {
                Some(value.0)
            }
        }
    };
}
scalar!(I1, i32, 0);
scalar!(I8, i32, 0);
scalar!(I16, i32, 0);
scalar!(I32, i32, 0);
scalar!(I64, i64, 0);
scalar!(F32, f32, 0.0);
scalar!(F64, f64, 0.0);

macro_rules! integer {
    ($rust:ty, $unsigned:ty, $carrier:ty) => {
        impl AbiValue for $rust {
            type Value = Self;
            #[inline]
            fn lower(value: Self) -> Carrier<Self> {
                // Signed narrow values cross the boundary as their logical bits.
                (value as $unsigned as $carrier, ())
            }
            #[inline]
            fn lift(value: Carrier<Self>) -> Option<Self> {
                Some(value.0 as $unsigned as Self)
            }
        }
    };
}
integer!(u8, u8, i32);
integer!(i8, u8, i32);
integer!(u16, u16, i32);
integer!(i16, u16, i32);
integer!(u32, u32, i32);
integer!(i32, u32, i32);
integer!(u64, u64, i64);
integer!(i64, u64, i64);

impl AbiValue for bool {
    type Value = Self;
    #[inline]
    fn lower(value: bool) -> Carrier<Self> {
        (i32::from(value), ())
    }
    #[inline]
    fn lift(value: Carrier<Self>) -> Option<bool> {
        match value.0 {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}

impl AbiValue for f32 {
    type Value = Self;
    #[inline]
    fn lower(value: f32) -> Carrier<Self> {
        (value, ())
    }
    #[inline]
    fn lift(value: Carrier<Self>) -> Option<f32> {
        Some(value.0)
    }
}

impl AbiValue for f64 {
    type Value = Self;
    #[inline]
    fn lower(value: f64) -> Carrier<Self> {
        (value, ())
    }
    #[inline]
    fn lift(value: Carrier<Self>) -> Option<f64> {
        Some(value.0)
    }
}

macro_rules! tuple {
    () => {};
    ($first:ident $first_value:ident $(, $rest:ident $rest_value:ident)*) => {
        tuple!($($rest $rest_value),*);

        impl<$first: WasmRepr, $($rest: WasmRepr),*> WasmRepr for ($first, $($rest,)*)
        where
            ($($rest,)*): WasmRepr,
            $first::Slots: Concat<<($($rest,)*) as WasmRepr>::Slots>,
        {
            type Slots = <$first::Slots as Concat<<($($rest,)*) as WasmRepr>::Slots>>::Output;
        }
        impl<$first: AbiValue, $($rest: AbiValue),*> AbiValue for ($first, $($rest,)*)
        where
            ($($rest,)*): AbiValue<Value = ($($rest::Value,)*), Logical = ($($rest::Logical,)*)>,
            ($($rest::Logical,)*): WasmRepr,
            Layout<$first>: Concat<Layout<($($rest,)*)>>,
        {
            type Value = ($first::Value, $($rest::Value,)*);
            #[inline]
            fn lower(values: Self::Value) -> Carrier<Self> {
                let ($first_value, $($rest_value,)*) = values;
                <Layout<$first> as Concat<_>>::concat(
                    $first::lower($first_value),
                    <($($rest,)*)>::lower(($($rest_value,)*)),
                )
            }
            #[inline]
            fn lift(values: Carrier<Self>) -> Option<Self::Value> {
                let (first, rest) = <Layout<$first> as Concat<_>>::split(values);
                let first = $first::lift(first)?;
                let ($($rest_value,)*) = <($($rest,)*)>::lift(rest)?;
                Some((first, $($rest_value,)*))
            }
        }
    };
}
tuple!(A a, B b, C c, D d, E e, F f, G g, H h, I i, J j, K k, L l);

macro_rules! repeat_type {
    ($_value:ident, $ty:ty) => {
        $ty
    };
}
macro_rules! array {
    ($count:literal; $($value:ident),*) => {
        impl<T: WasmRepr> WasmRepr for [T; $count]
        where
            ($(repeat_type!($value, T),)*): WasmRepr,
        {
            type Slots = <($(repeat_type!($value, T),)*) as WasmRepr>::Slots;
        }
        impl<T: AbiValue> AbiValue for [T; $count]
        where
            ($(repeat_type!($value, T),)*): AbiValue<Value = ($(repeat_type!($value, T::Value),)*), Logical = ($(repeat_type!($value, T::Logical),)*)>,
            ($(repeat_type!($value, T::Logical),)*): WasmRepr,
        {
            type Value = [T::Value; $count];
            #[inline]
            fn lower(values: Self::Value) -> Carrier<Self> {
                let [$($value,)*] = values;
                <($(repeat_type!($value, T),)*)>::lower(($($value,)*))
            }
            #[inline]
            fn lift(values: Carrier<Self>) -> Option<Self::Value> {
                let ($($value,)*) = <($(repeat_type!($value, T),)*)>::lift(values)?;
                Some([$($value,)*])
            }
        }
    };
}
array!(0;);
array!(1; a);
array!(2; a, b);
array!(3; a, b, c);
array!(4; a, b, c, d);
array!(5; a, b, c, d, e);
array!(6; a, b, c, d, e, f);
array!(7; a, b, c, d, e, f, g);
array!(8; a, b, c, d, e, f, g, h);
array!(9; a, b, c, d, e, f, g, h, i);
array!(10; a, b, c, d, e, f, g, h, i, j);
array!(11; a, b, c, d, e, f, g, h, i, j, k);
array!(12; a, b, c, d, e, f, g, h, i, j, k, l);

/// A flat Rust tuple of the physical Wasm carriers for an ABI type.
pub type Flat<T> = <Layout<T> as Tuple>::Tuple;

/// Lowers a value into the physical tuple accepted by a native engine.
#[inline]
pub fn lower_flat<T: AbiValue<Logical: WasmRepr<Slots: Tuple>>>(value: Value<T>) -> Flat<T> {
    <Layout<T> as Tuple>::into_tuple(T::lower(value))
}

/// Decodes a physical tuple, rejecting values outside the type's ABI contract.
#[inline]
pub fn lift_flat<T: AbiValue<Logical: WasmRepr<Slots: Tuple>>>(value: Flat<T>) -> Option<Value<T>> {
    T::lift(<Layout<T> as Tuple>::from_tuple(value))
}
