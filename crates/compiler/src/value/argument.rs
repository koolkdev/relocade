//! Adapts typed values and native literals to runtime function signatures.

use super::{Val, ValueSource};
use crate::{arena::FunctionArena, BuildError, Type, ValueType};

/// A value or literal supplied where a function signature determines its type.
/// Typed values, including typed literals, keep their logical type and any body
/// ownership and branch visibility. Native literals supplied directly use the expected type.
/// Signed i32 literals sign-extend to I64; u32 literals zero-extend. Both reduce
/// to the low bits for narrower types. A u64 literal requires I64 or V64; bool requires I1.
/// An f64 literal requires F64; integer literals never implicitly become floating values.
/// A u128 or 16-byte array requires V128; an 8-byte array requires V64.
/// Array bytes use increasing memory order.
/// Other byte-array widths do not construct vector literals:
/// ```compile_fail
/// use relocade_compiler::Argument;
/// let vector = Argument::from([0_u8; 4]);
/// ```
#[derive(Clone)]
pub struct Argument(Operand);

#[derive(Clone)]
enum Operand {
    Value { source: ValueSource, ty: Type },
    Signed(i32),
    Unsigned(u32),
    Wide(u64),
    Bit(bool),
    Float(u64),
}

impl Argument {
    pub(crate) fn resolve(
        &self,
        arena: &FunctionArena,
        expected: Type,
        scope: usize,
    ) -> Result<usize, BuildError> {
        let bits = match &self.0 {
            Operand::Value { source, ty } => {
                let value = source.resolve(arena, *ty)?;
                if *ty != expected {
                    return Err(BuildError::TypeMismatch {
                        expected,
                        actual: *ty,
                    });
                }
                return value.admit(arena, scope);
            }
            Operand::Signed(value) if expected.is_integer() => *value as i64 as u64,
            Operand::Unsigned(value) if expected.is_integer() => u64::from(*value),
            Operand::Wide(value) if matches!(expected, Type::I64 | Type::V64) => *value,
            Operand::Bit(value) if expected == Type::I1 => u64::from(*value),
            Operand::Float(bits) if expected == Type::F64 => *bits,
            Operand::Signed(_) | Operand::Unsigned(_) => {
                return Err(BuildError::TypeMismatch {
                    expected,
                    actual: Type::I32,
                })
            }
            Operand::Float(_) => {
                return Err(BuildError::TypeMismatch {
                    expected,
                    actual: Type::F64,
                })
            }
            Operand::Wide(_) => {
                return Err(BuildError::TypeMismatch {
                    expected,
                    actual: Type::I64,
                })
            }
            Operand::Bit(_) => {
                return Err(BuildError::TypeMismatch {
                    expected,
                    actual: Type::I1,
                })
            }
        };
        arena.literal(expected, bits)
    }
}

impl<T: ValueType> From<&Val<T>> for Argument {
    fn from(value: &Val<T>) -> Self {
        Self(Operand::Value {
            source: value.clone().into_source(),
            ty: T::TYPE,
        })
    }
}

impl<T: ValueType> From<Val<T>> for Argument {
    fn from(value: Val<T>) -> Self {
        Self(Operand::Value {
            source: value.into_source(),
            ty: T::TYPE,
        })
    }
}

impl From<i32> for Argument {
    fn from(value: i32) -> Self {
        Self(Operand::Signed(value))
    }
}
impl From<u32> for Argument {
    fn from(value: u32) -> Self {
        Self(Operand::Unsigned(value))
    }
}
impl From<u64> for Argument {
    fn from(value: u64) -> Self {
        Self(Operand::Wide(value))
    }
}
impl From<bool> for Argument {
    fn from(value: bool) -> Self {
        Self(Operand::Bit(value))
    }
}

impl From<f64> for Argument {
    fn from(value: f64) -> Self {
        Self(Operand::Float(value.to_bits()))
    }
}

impl From<u128> for Argument {
    fn from(bits: u128) -> Self {
        Val::<crate::V128>::from(bits).into()
    }
}

impl<const N: usize> From<[u8; N]> for Argument
where
    crate::literal::Literal: From<[u8; N]>,
{
    fn from(bytes: [u8; N]) -> Self {
        let ty = match N {
            8 => Type::V64,
            16 => Type::V128,
            _ => unreachable!("supported vector literal width"),
        };
        Self(Operand::Value {
            source: ValueSource::Literal(bytes.into()),
            ty,
        })
    }
}
