//! Adapts typed values and native literals to runtime function signatures.

use super::{Val, ValueSource};
use crate::{arena::ExpressionArena, BuildError, IntType, Type};

/// An integer value or literal supplied where a function signature determines its type.
/// Typed values, including typed literals, keep their logical type and any body
/// ownership and branch visibility. Native literals supplied directly use the expected type.
/// Signed i32 literals sign-extend to I64; u32 literals zero-extend. Both reduce
/// to the low bits for narrower types. A u64 literal requires I64, and bool requires I1.
#[derive(Clone)]
pub struct Argument(Operand);

#[derive(Clone)]
enum Operand {
    Value { source: ValueSource, ty: Type },
    Signed(i32),
    Unsigned(u32),
    Wide(u64),
    Bit(bool),
}

impl Argument {
    pub(crate) fn resolve(
        &self,
        arena: &ExpressionArena,
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
            Operand::Signed(value) => *value as i64 as u64,
            Operand::Unsigned(value) => u64::from(*value),
            Operand::Wide(value) if expected == Type::I64 => *value,
            Operand::Bit(value) if expected == Type::I1 => u64::from(*value),
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
        arena.constant(expected, bits)
    }
}

impl<T: IntType> From<&Val<T>> for Argument {
    fn from(value: &Val<T>) -> Self {
        Self(Operand::Value {
            source: value.source.clone(),
            ty: T::TYPE,
        })
    }
}

impl<T: IntType> From<Val<T>> for Argument {
    fn from(value: Val<T>) -> Self {
        Self(Operand::Value {
            source: value.source,
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
