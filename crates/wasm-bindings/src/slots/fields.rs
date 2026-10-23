//! Packs the separately converted fields of an aggregate into its scalar slots.
use super::{Concat, ConcatValues, Native, Slots, Values};

/// Joins a tuple of field layouts into the aggregate's scalar layout.
pub trait Fields {
    type Slots: Slots;
}
impl Fields for () {
    type Slots = ();
}
impl<H: Slots> Fields for (H,) {
    type Slots = H;
}

/// Flattens already converted fields, preserving each field's codec contract.
pub trait Pack<R = Native>: Fields<Slots: Slots<R>> {
    type Values;
    fn pack(values: Self::Values) -> Values<Self::Slots, R>;
    fn unpack(values: Values<Self::Slots, R>) -> Self::Values;
}
impl<R> Pack<R> for () {
    type Values = ();
    fn pack(_: ()) {}
    fn unpack(_: ()) {}
}
impl<H: Slots + Slots<R>, R> Pack<R> for (H,) {
    type Values = (Values<H, R>,);
    fn pack(values: Self::Values) -> Values<H, R> {
        values.0
    }
    fn unpack(values: Values<H, R>) -> Self::Values {
        (values,)
    }
}

macro_rules! tuple {
    ($head:ident $head_value:ident) => {};
    ($head:ident $head_value:ident $(, $tail:ident $tail_value:ident)+) => {
        impl<$head: Slots, $($tail: Slots),*> Fields for ($head, $($tail,)*)
        where
            ($($tail,)*): Fields,
            $head: Concat<<($($tail,)*) as Fields>::Slots>,
        {
            type Slots = <$head as Concat<<($($tail,)*) as Fields>::Slots>>::Output;
        }
        impl<R, $head: Slots + Slots<R>, $($tail: Slots + Slots<R>),*> Pack<R> for ($head, $($tail,)*)
        where
            ($($tail,)*): Pack<R, Values = ($(Values<$tail, R>,)*), Slots: Slots<R>>,
            $head: ConcatValues<<($($tail,)*) as Fields>::Slots, R, Output: Slots<R>>,
        {
            type Values = (Values<$head, R>, $(Values<$tail, R>,)*);
            fn pack(values: Self::Values) -> Values<Self::Slots, R> {
                let ($head_value, $($tail_value,)*) = values;
                $head::concat($head_value, <($($tail,)*) as Pack<R>>::pack(($($tail_value,)*)))
            }
            fn unpack(values: Values<Self::Slots, R>) -> Self::Values {
                let (head, tail) = $head::split(values);
                let ($($tail_value,)*) = <($($tail,)*) as Pack<R>>::unpack(tail);
                (head, $($tail_value,)*)
            }
        }
        tuple!($($tail $tail_value),*);
    };
}
tuple!(A a, B b, C c, D d, E e, F f, G g, H h, I i, J j, K k, L l);
