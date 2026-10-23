use super::{
    slots::{self, Ir, Layout, Scalar, Slots},
    CompilerValue,
};
use crate::{Results, Val, F64, I1, I16, I32, I64, I8};
use relocade_wasm_bindings::{
    slots::{Fields, Pack, Slots as PortableSlots},
    AbiValue, WasmRepr,
};

/// Projects logical IR values into the scalar layout chosen by the portable ABI.
pub trait WasmValues: Results + WasmRepr<Slots: Slots> {
    fn flatten_values(values: Self::Values) -> <Self::Slots as Results>::Values;
    fn unflatten_values(values: <Self::Slots as Results>::Values) -> Self::Values;
}
impl WasmValues for () {
    fn flatten_values(_: ()) {}
    fn unflatten_values(_: ()) {}
}
macro_rules! scalar {
    ($logical:ty, $carrier:ty, $literal:ty) => {
        impl Scalar for $logical {
            fn constant(value: $carrier) -> Val<Self> {
                (value as $literal).into()
            }
        }
        impl WasmValues for $logical {
            fn flatten_values(value: Val<Self>) -> (Val<Self>, ()) {
                (value, ())
            }
            fn unflatten_values(value: (Val<Self>, ())) -> Val<Self> {
                value.0
            }
        }
    };
}
scalar!(I1, i32, u32);
scalar!(I8, i32, u32);
scalar!(I16, i32, u32);
scalar!(I32, i32, u32);
scalar!(I64, i64, u64);
scalar!(F64, f64, f64);

macro_rules! tuple {
    () => {};
    (@rest $first:ident $value:ident $(, $rest:ident $rest_value:ident)*) => {
        tuple!($($rest $rest_value),*);
    };
    ($($element:ident $value:ident),+) => {
        impl<$($element: WasmValues),+> WasmValues for ($($element,)+)
        where
            ($($element::Slots,)+): Pack<Ir, Values = ($(<$element::Slots as Results>::Values,)+)>,
            <($($element::Slots,)+) as Fields>::Slots: Slots,
        {
            fn flatten_values(values: Self::Values) -> <Self::Slots as Results>::Values {
                let ($($value,)+) = values;
                <($($element::Slots,)+) as Pack<Ir>>::pack(($($element::flatten_values($value),)+))
            }
            fn unflatten_values(values: <Self::Slots as Results>::Values) -> Self::Values {
                let ($($value,)+) = <($($element::Slots,)+) as Pack<Ir>>::unpack(values);
                ($($element::unflatten_values($value),)+)
            }
        }
        tuple!(@rest $($element $value),+);
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
        impl<T: WasmValues> WasmValues for [T; $count]
        where
            ($(repeat_type!($value, T),)*): WasmValues + Results<Values = ($(repeat_type!($value, T::Values),)*)>,
        {
            fn flatten_values(values: Self::Values) -> <Self::Slots as Results>::Values {
                let [$($value,)*] = values;
                <($(repeat_type!($value, T),)*)>::flatten_values(($($value,)*))
            }
            fn unflatten_values(values: <Self::Slots as Results>::Values) -> Self::Values {
                let ($($value,)*) = <($(repeat_type!($value, T),)*)>::unflatten_values(values);
                [$($value,)*]
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

/// Constructs symbolic constants using the portable Rust codec and layout.
pub fn constant<T: AbiValue + CompilerValue<Logical: WasmValues>>(value: T::Value) -> T::Values {
    slots::from_value_slots::<T>(<Layout<T> as PortableSlots<Ir>>::constants(T::lower(value)))
}
