//! Addition and subtraction results, carry inputs and flag equations.

use wasm86_compiler::{MemoryInt, Val, I1};

use super::{
    bit,
    flags::{AnyFlagSource, FlagChange, FlagSource, StatusFlag},
    result_flag, AluResult,
};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum ArithmeticOp {
    Add,
    Subtract,
}

impl ArithmeticOp {
    /// Also reconstructs the result needed by flags decoded from stored operands.
    pub(crate) fn result<T: MemoryInt>(self, left: &Val<T>, right: &Val<T>) -> Val<T> {
        match self {
            Self::Add => left.add(right),
            Self::Subtract => left.sub(right),
        }
    }

    pub(crate) fn apply<T: MemoryInt>(
        self,
        left: impl Into<Val<T>>,
        right: impl Into<Val<T>>,
    ) -> AluResult<T>
    where
        FlagSource<T>: Into<AnyFlagSource>,
    {
        let left = left.into();
        let right = right.into();
        let result = self.result(&left, &right);
        let flags = FlagSource::Arithmetic {
            operation: self,
            left,
            right,
            result: result.clone(),
        };
        AluResult {
            result,
            flags: FlagChange::from(flags),
        }
    }

    pub(crate) fn apply_with_carry<T: MemoryInt>(
        self,
        left: impl Into<Val<T>>,
        right: impl Into<Val<T>>,
        carry_in: Val<I1>,
    ) -> AluResult<T>
    where
        FlagSource<T>: Into<AnyFlagSource>,
    {
        let left = left.into();
        let right = right.into();
        let intermediate = self.result(&left, &right);
        let carry_operand = carry_in.unsigned().extend::<T>();
        let result = self.result(&intermediate, &carry_operand);
        let flags = StatusFlag::ALL.map(|flag| self.flag(&left, &right, &result, &carry_in, flag));
        AluResult {
            result,
            flags: FlagChange::from(FlagSource::<T>::Explicit { flags }),
        }
    }

    pub(super) fn flag<T: MemoryInt>(
        self,
        left: &Val<T>,
        right: &Val<T>,
        result: &Val<T>,
        carry_in: &Val<I1>,
        flag: StatusFlag,
    ) -> Val<I1> {
        match flag {
            StatusFlag::CF => {
                // Addition compares its wrapped result; subtraction compares its
                // operands. Incoming carry or borrow decides the equality case.
                let (left, right) = match self {
                    Self::Add => (result, left),
                    Self::Subtract => (left, right),
                };
                carry_in.select(right.unsigned().ge(left), left.unsigned().lt(right))
            }
            StatusFlag::AF => bit(&left.xor(right).xor(result), 4),
            StatusFlag::OF => {
                let left_xor_result = left.xor(result);
                let overflow_pair = match self {
                    Self::Add => right.xor(result),
                    Self::Subtract => left.xor(right),
                };
                bit(&left_xor_result.and(overflow_pair), T::BYTES * 8 - 1)
            }
            StatusFlag::PF | StatusFlag::ZF | StatusFlag::SF => result_flag(result, flag),
        }
    }
}
