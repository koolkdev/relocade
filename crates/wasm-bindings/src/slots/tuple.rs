//! Conversion from scalar slot lists to the Rust tuples accepted by engines.

use super::{Cons, Scalar, Slots};

/// Converts a flat slot list into the tuple accepted by an execution engine.
pub trait Tuple: Slots {
    type Tuple: 'static;
    fn into_tuple(values: Self::Carrier) -> Self::Tuple;
    fn from_tuple(values: Self::Tuple) -> Self::Carrier;
}

impl Tuple for () {
    type Tuple = ();
    fn into_tuple(_: ()) {}
    fn from_tuple(_: ()) {}
}

macro_rules! list {
    () => { () };
    ($head:ident $(, $tail:ident)*) => { Cons<$head, list!($($tail),*)> };
}
macro_rules! nested {
    () => { () };
    ($head:ident $(, $tail:ident)*) => { ($head, nested!($($tail),*)) };
}
macro_rules! tuple {
    () => {};
    (@rest $first:ident $value:ident $(, $rest:ident $rest_value:ident)*) => {
        tuple!($($rest $rest_value),*);
    };
    ($($element:ident $value:ident),+) => {
        impl<$($element: Scalar),+> Tuple for list!($($element),+) {
            type Tuple = ($($element::Carrier,)+);
            fn into_tuple(values: Self::Carrier) -> Self::Tuple {
                let nested!($($value),+) = values;
                ($($value,)+)
            }
            fn from_tuple(values: Self::Tuple) -> Self::Carrier {
                let ($($value,)+) = values;
                nested!($($value),+)
            }
        }
        tuple!(@rest $($element $value),+);
    };
}

tuple!(A a, B b, C c, D d, E e, F f, G g, H h, I i, J j, K k, L l);
