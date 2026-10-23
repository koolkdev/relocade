//! Ordered scalar layouts shared by Rust codecs and generated code.
mod fields;
mod tuple;
use crate::WasmRepr;
use core::marker::PhantomData;
pub use fields::{Fields, Pack};
pub use tuple::Tuple;

/// One logical scalar followed by the remaining slots. `()` ends the list.
/// This is a type-level layout, not a Rust memory representation.
pub struct Cons<H, T>(PhantomData<(H, T)>);

pub trait Scalar: 'static {
    type Carrier: 'static;
    fn zero() -> Self::Carrier;
}

/// Native Wasm carriers; the compiler supplies its own representation.
pub struct Native;

/// Constructs a scalar in a particular execution representation.
pub trait Representation<S: Scalar> {
    type Value: 'static;
    fn constant(value: S::Carrier) -> Self::Value;
}
impl<S: Scalar> Representation<S> for Native {
    type Value = S::Carrier;
    fn constant(value: S::Carrier) -> Self::Value {
        value
    }
}

/// The values carried by a slot layout in one execution representation.
pub type Values<S, R = Native> = <S as Slots<R>>::Carrier;

/// Walks a layout to construct zeros or constants in that representation.
pub trait Slots<R = Native>: WasmRepr<Slots = Self> + Sized + 'static {
    type Carrier: 'static;
    fn zero() -> Self::Carrier;
    fn constants(values: <Self as Slots>::Carrier) -> Values<Self, R>
    where
        Self: Slots;
}
impl<R> Slots<R> for () {
    type Carrier = ();
    fn zero() {}
    fn constants(_: ()) {}
}
impl<H: Scalar, T: Slots> WasmRepr for Cons<H, T> {
    type Slots = Self;
}
impl<H: Scalar, T: Slots + Slots<R>, R: Representation<H>> Slots<R> for Cons<H, T> {
    type Carrier = (R::Value, <T as Slots<R>>::Carrier);
    fn zero() -> Self::Carrier {
        (R::constant(H::zero()), <T as Slots<R>>::zero())
    }
    fn constants(values: <Self as Slots>::Carrier) -> Self::Carrier {
        (R::constant(values.0), <T as Slots<R>>::constants(values.1))
    }
}

/// Concatenates field layouts independently of their execution representation.
pub trait Concat<R: Slots>: Slots {
    type Output: Slots;
}
impl<R: Slots> Concat<R> for () {
    type Output = R;
}
impl<H: Scalar, T: Slots + Concat<R>, R: Slots> Concat<R> for Cons<H, T> {
    type Output = Cons<H, T::Output>;
}

/// Concatenation and its inverse use the same traversal for every representation.
pub trait ConcatValues<R: Slots + Slots<V>, V = Native>:
    Concat<R, Output: Slots<V>> + Slots<V>
{
    fn concat(left: Values<Self, V>, right: Values<R, V>) -> Values<Self::Output, V>;
    fn split(value: Values<Self::Output, V>) -> (Values<Self, V>, Values<R, V>);
}
impl<R: Slots + Slots<V>, V> ConcatValues<R, V> for () {
    fn concat(_: (), right: Values<R, V>) -> Values<R, V> {
        right
    }
    fn split(value: Values<R, V>) -> ((), Values<R, V>) {
        ((), value)
    }
}
impl<H: Scalar, T: Slots + ConcatValues<R, V>, R: Slots + Slots<V>, V: Representation<H>>
    ConcatValues<R, V> for Cons<H, T>
where
    T::Output: Slots<V>,
{
    fn concat(left: Values<Self, V>, right: Values<R, V>) -> Values<Self::Output, V> {
        (left.0, <T as ConcatValues<R, V>>::concat(left.1, right))
    }
    fn split(value: Values<Self::Output, V>) -> (Values<Self, V>, Values<R, V>) {
        let (tail, right) = <T as ConcatValues<R, V>>::split(value.1);
        ((value.0, tail), right)
    }
}

/// Flattened logical layout of an ABI type.
pub type Layout<T> = <<T as crate::AbiType>::Logical as WasmRepr>::Slots;
