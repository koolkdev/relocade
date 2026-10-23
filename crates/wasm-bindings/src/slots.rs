//! Ordered scalar layouts shared by Rust codecs and generated code.
mod tuple;
use crate::WasmRepr;
use core::marker::PhantomData;
pub use tuple::Tuple;

/// One logical scalar followed by the remaining slots. `()` ends the list.
/// This is a type-level layout, not a Rust memory representation.
pub struct Cons<H, T>(PhantomData<(H, T)>);

pub trait Scalar: 'static {
    type Carrier: 'static;
    fn zero() -> Self::Carrier;
}

pub trait Slots: WasmRepr<Slots = Self> + Sized + 'static {
    type Carrier: 'static;
    fn zero() -> Self::Carrier;
}
impl Slots for () {
    type Carrier = ();
    #[inline]
    fn zero() {}
}
impl<H: Scalar, T: Slots> WasmRepr for Cons<H, T> {
    type Slots = Self;
}
impl<H: Scalar, T: Slots> Slots for Cons<H, T> {
    type Carrier = (H::Carrier, T::Carrier);
    #[inline]
    fn zero() -> Self::Carrier {
        (H::zero(), T::zero())
    }
}

/// Concatenates field layouts, with inverse operations for their carriers.
pub trait Concat<R: Slots>: Slots {
    type Output: Slots;
    fn concat(left: Self::Carrier, right: R::Carrier) -> <Self::Output as Slots>::Carrier;
    fn split(value: <Self::Output as Slots>::Carrier) -> (Self::Carrier, R::Carrier);
}
impl<R: Slots> Concat<R> for () {
    type Output = R;
    fn concat(_: (), right: R::Carrier) -> R::Carrier {
        right
    }
    fn split(value: R::Carrier) -> ((), R::Carrier) {
        ((), value)
    }
}
impl<H: Scalar, T: Slots + Concat<R>, R: Slots> Concat<R> for Cons<H, T> {
    type Output = Cons<H, T::Output>;
    fn concat(left: Self::Carrier, right: R::Carrier) -> <Self::Output as Slots>::Carrier {
        (left.0, T::concat(left.1, right))
    }
    fn split(value: <Self::Output as Slots>::Carrier) -> (Self::Carrier, R::Carrier) {
        let (tail, right) = T::split(value.1);
        ((value.0, tail), right)
    }
}

/// Flattened logical layout of an ABI type.
pub type Layout<T> = <<T as crate::AbiType>::Logical as WasmRepr>::Slots;
