/// Logical integer types used by values and function signatures.
///
/// These describe integer bit patterns. Operations choose signed or unsigned
/// interpretation; WebAssembly storage is chosen during emission.
///
/// At function boundaries, I1, I8 and I16 use zero-extended Wasm i32 values.
/// Callers must supply arguments in 0..=1, 0..=255 and 0..=65535 respectively.
/// Narrow return values are zero-extended to those same ranges. Imported functions
/// must follow this contract too, including when they are exported directly.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Type {
    I1,
    I8,
    I16,
    I32,
    I64,
}

impl Type {
    pub(super) fn bits(self) -> u8 {
        match self {
            Self::I1 => 1,
            Self::I8 => 8,
            Self::I16 => 16,
            Self::I32 => 32,
            Self::I64 => 64,
        }
    }

    pub(super) fn normalize(self, bits: u64) -> u64 {
        bits & self.mask()
    }

    pub(super) fn mask(self) -> u64 {
        match self {
            Self::I1 => 1,
            Self::I8 => 0xff,
            Self::I16 => 0xffff,
            Self::I32 => 0xffff_ffff,
            Self::I64 => u64::MAX,
        }
    }
}

mod sealed {
    pub trait Sealed {}
}

/// A supported integer type known at compile time.
pub trait IntType: Copy + sealed::Sealed + 'static {
    const TYPE: Type;
}

/// A logical one-bit integer type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct I1;

/// A logical 8-bit integer type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct I8;

/// A logical 16-bit integer type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct I16;

/// A logical 32-bit integer type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct I32;

/// A logical 64-bit integer type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct I64;

impl sealed::Sealed for I1 {}
impl sealed::Sealed for I8 {}
impl sealed::Sealed for I16 {}
impl sealed::Sealed for I32 {}
impl sealed::Sealed for I64 {}

impl IntType for I1 {
    const TYPE: Type = Type::I1;
}

impl IntType for I8 {
    const TYPE: Type = Type::I8;
}

impl IntType for I16 {
    const TYPE: Type = Type::I16;
}

impl IntType for I32 {
    const TYPE: Type = Type::I32;
}

impl IntType for I64 {
    const TYPE: Type = Type::I64;
}

/// Integer types whose bit count is at least that of `Other`.
/// Extension and truncation use this relation to check their direction in Rust.
/// A type is at least as wide as itself, so identity conversions are allowed.
pub trait AtLeast<Other: IntType>: IntType {}

macro_rules! at_least {
    ($wide:ty: $($narrow:ty),+ $(,)?) => {$(impl AtLeast<$narrow> for $wide {})+};
}

at_least!(I1: I1);
at_least!(I8: I1, I8);
at_least!(I16: I1, I8, I16);
at_least!(I32: I1, I8, I16, I32);
at_least!(I64: I1, I8, I16, I32, I64);
