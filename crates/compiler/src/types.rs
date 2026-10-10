/// Logical scalar types used by values and function signatures.
///
/// Integer operations choose signed or unsigned interpretation. F64 uses strict
/// IEEE binary64 arithmetic. WebAssembly storage is chosen during emission.
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
    F64,
}

impl Type {
    /// The Wasm scalar type carrying this logical value.
    pub(super) fn carrier(self) -> Self {
        match self {
            Self::I1 | Self::I8 | Self::I16 | Self::I32 => Self::I32,
            Self::I64 | Self::F64 => self,
        }
    }

    pub(super) fn is_integer(self) -> bool {
        matches!(
            self,
            Self::I1 | Self::I8 | Self::I16 | Self::I32 | Self::I64
        )
    }

    /// Scalar encodings fit in u64, including the raw bits of F64.
    pub(super) fn is_scalar(self) -> bool {
        match self {
            Self::I1 | Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::F64 => true,
        }
    }

    pub(super) fn bits(self) -> u8 {
        match self {
            Self::I1 => 1,
            Self::I8 => 8,
            Self::I16 => 16,
            Self::I32 => 32,
            Self::I64 | Self::F64 => 64,
        }
    }

    /// Keep the logical bits of a scalar encoding.
    pub(super) fn normalize(self, bits: u64) -> u64 {
        bits & self.mask()
    }

    /// The logical bit mask of a scalar encoding.
    pub(super) fn mask(self) -> u64 {
        debug_assert!(self.is_scalar(), "scalar masks require a scalar type");
        crate::integer::low_mask(self.bits())
    }
}

mod sealed {
    pub trait Sealed {
        // Typed handles retain only the encoding their logical type can carry.
        type Literal: Copy + Eq + Into<crate::literal::Literal> + From<crate::literal::Literal>;
    }
}

/// A supported scalar type known at compile time.
pub trait ValueType: Copy + sealed::Sealed + 'static {
    const TYPE: Type;
}

/// A value type supporting bitwise AND, OR and XOR.
pub trait BitwiseType: ValueType {}

/// A scalar integer supporting arithmetic and signed or unsigned views.
pub trait IntType: BitwiseType {}

impl BitwiseType for I1 {}
impl BitwiseType for I8 {}
impl BitwiseType for I16 {}
impl BitwiseType for I32 {}
impl BitwiseType for I64 {}

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

/// An IEEE binary64 floating-point type, carried by WebAssembly f64.
///
/// Floating values use the same parameters, calls, memory and result shapes as
/// integers. Their arithmetic uses scalar Wasm rounding and NaN behavior.
///
/// ```
/// use wasm86_compiler::{Program, Signature, Type, F64};
/// let mut program = Program::new();
/// let scale = program.function(Signature {
///     parameters: vec![Type::F64], results: vec![Type::F64],
/// }, |body| {
///     let value = body.parameter::<F64>(0)?;
///     body.return_(value.mul(0.5))
/// })?;
/// program.export("scale", scale)?;
/// let bytes = program.compile()?;
/// # Ok::<(), wasm86_compiler::BuildError>(())
/// ```
/// Integer bit operations require an explicit encoding view:
/// ```compile_fail
/// use wasm86_compiler::{Val, F64};
/// let bits = Val::<F64>::from(1.0).and(7);
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct F64;

impl sealed::Sealed for I1 {
    type Literal = u64;
}
impl sealed::Sealed for I8 {
    type Literal = u64;
}
impl sealed::Sealed for I16 {
    type Literal = u64;
}
impl sealed::Sealed for I32 {
    type Literal = u64;
}
impl sealed::Sealed for I64 {
    type Literal = u64;
}
impl sealed::Sealed for F64 {
    type Literal = u64;
}

impl ValueType for I1 {
    const TYPE: Type = Type::I1;
}

impl ValueType for I8 {
    const TYPE: Type = Type::I8;
}

impl ValueType for I16 {
    const TYPE: Type = Type::I16;
}

impl ValueType for I32 {
    const TYPE: Type = Type::I32;
}

impl ValueType for I64 {
    const TYPE: Type = Type::I64;
}

impl ValueType for F64 {
    const TYPE: Type = Type::F64;
}

impl IntType for I1 {}
impl IntType for I8 {}
impl IntType for I16 {}
impl IntType for I32 {}
impl IntType for I64 {}

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
// Every supported integer fits in I64, including an otherwise generic IntType.
impl<T: IntType> AtLeast<T> for I64 {}

/// An integer type with a supported scalar type exactly twice as wide.
///
/// I8 widens to I16, I16 to I32, and I32 to I64. I1 and I64 have no supported
/// double-width scalar type. Both types support memory access at their logical width.
pub trait DoubleWidth: crate::MemoryInt {
    type Double: crate::MemoryInt + AtLeast<Self> + AtLeast<I16>;
}

impl DoubleWidth for I8 {
    type Double = I16;
}

impl DoubleWidth for I16 {
    type Double = I32;
}

impl DoubleWidth for I32 {
    type Double = I64;
}
