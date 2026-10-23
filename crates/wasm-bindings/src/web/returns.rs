//! The Rust C ABI representation of a flattened result list.
//!
//! Unit and one scalar return directly. Rust lowers larger C aggregates through
//! a hidden return pointer; generated adapters use these exact field offsets.

use wasm_encoder::ValType;

pub(super) trait Scalar: Copy + 'static {
    const TYPE: ValType;
}
impl Scalar for i32 {
    const TYPE: ValType = ValType::I32;
}
impl Scalar for i64 {
    const TYPE: ValType = ValType::I64;
}
impl Scalar for f32 {
    const TYPE: ValType = ValType::F32;
}
impl Scalar for f64 {
    const TYPE: ValType = ValType::F64;
}

#[derive(Hash, PartialEq, Eq)]
pub(super) struct Area {
    pub offsets: Vec<u64>,
    pub size: u32,
}

pub(super) struct Layout {
    pub types: Vec<ValType>,
    pub area: Option<Area>,
}

pub(super) trait Returns: Sized + 'static {
    type Abi;
    fn layout() -> Layout;
    fn into_abi(self) -> Self::Abi;
    fn from_abi(value: Self::Abi) -> Self;
}

impl Returns for () {
    type Abi = ();
    fn layout() -> Layout {
        Layout {
            types: vec![],
            area: None,
        }
    }
    fn into_abi(self) {}
    fn from_abi(_: ()) {}
}

impl<A: Scalar> Returns for (A,) {
    type Abi = A;
    fn layout() -> Layout {
        Layout {
            types: vec![A::TYPE],
            area: None,
        }
    }
    fn into_abi(self) -> A {
        self.0
    }
    fn from_abi(value: A) -> Self {
        (value,)
    }
}

macro_rules! aggregate {
    ($name:ident; $($field:ident $index:tt),+) => {
        #[repr(C)]
        pub(super) struct $name<$($field: Scalar),+>($(pub $field),+);
        impl<$($field: Scalar),+> Returns for ($($field,)+) {
            type Abi = $name<$($field),+>;
            fn layout() -> Layout {
                Layout {
                    types: vec![$($field::TYPE),+],
                    area: Some(Area {
                        offsets: vec![$(std::mem::offset_of!(Self::Abi, $index) as u64),+],
                        size: std::mem::size_of::<Self::Abi>() as u32,
                    }),
                }
            }
            fn into_abi(self) -> Self::Abi { $name($(self.$index),+) }
            fn from_abi(value: Self::Abi) -> Self { ($(value.$index,)+) }
        }
    };
}
aggregate!(Return2; A 0, B 1);
aggregate!(Return3; A 0, B 1, C 2);
aggregate!(Return4; A 0, B 1, C 2, D 3);
aggregate!(Return5; A 0, B 1, C 2, D 3, E 4);
aggregate!(Return6; A 0, B 1, C 2, D 3, E 4, F 5);
aggregate!(Return7; A 0, B 1, C 2, D 3, E 4, F 5, G 6);
aggregate!(Return8; A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
aggregate!(Return9; A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);
aggregate!(Return10; A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9);
aggregate!(Return11; A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10);
aggregate!(Return12; A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11);
