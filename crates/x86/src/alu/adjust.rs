//! Packed and unpacked decimal adjustments and immediate-base digit conversions.
//! Architecturally undefined status flags retain their incoming values.

use wasm86_compiler::{Val, I1, I16, I8};

use super::{result_flag, AluResult, ArithmeticOp};
use crate::flags::{Flag, FlagChange, StatusFlag};

impl ArithmeticOp {
    pub(crate) fn adjust_unpacked(
        self,
        accumulator: Val<I16>,
        auxiliary: Val<I1>,
    ) -> AluResult<I16> {
        let adjust = accumulator.and(0xf).unsigned().ge(10).or(auxiliary);
        let correction = adjust.select(0x106, 0);
        // Intel specifies word arithmetic here: the low-byte correction can
        // carry or borrow into AH before its separate increment or decrement.
        let result = self.result(&accumulator, &correction).and(0xff0f);
        let flags = FlagChange::partial([(Flag::AF, adjust.clone()), (Flag::CF, adjust)]);
        AluResult { result, flags }
    }

    pub(crate) fn adjust_packed(
        self,
        accumulator: Val<I8>,
        auxiliary: Val<I1>,
        carry: Val<I1>,
    ) -> AluResult<I8> {
        let adjust_low = accumulator.and(0xf).unsigned().ge(10).or(auxiliary);
        // Both decisions use the original AL, before either digit is adjusted.
        let adjust_high = accumulator.unsigned().ge(0x9a).or(carry);
        let low_correction: Val<I8> = adjust_low.select(6, 0);
        let correction = low_correction.add(adjust_high.select(0x60, 0));
        let result = self.result(&accumulator, &correction);
        let carry = match self {
            Self::Add => adjust_high,
            // DAS retains a low-digit borrow even without a high correction.
            Self::Subtract => adjust_high.or(adjust_low.and(accumulator.unsigned().lt(6))),
        };
        let flags = FlagChange::partial([
            (Flag::AF, adjust_low),
            (Flag::CF, carry),
            (Flag::PF, result_flag(&result, StatusFlag::PF)),
            (Flag::ZF, result_flag(&result, StatusFlag::ZF)),
            (Flag::SF, result_flag(&result, StatusFlag::SF)),
        ]);
        AluResult { result, flags }
    }
}

/// The caller must raise #DE for a zero base before consuming these expressions.
pub(crate) fn adjust_after_multiply(accumulator: Val<I8>, base: Val<I8>) -> AluResult<I16> {
    let high = accumulator.unsigned().div(&base);
    let low = accumulator.unsigned().rem(base);
    AluResult {
        result: high
            .unsigned()
            .extend::<I16>()
            .shl(8)
            .or(low.unsigned().extend::<I16>()),
        flags: digit_flags(&low),
    }
}

pub(crate) fn adjust_before_division(accumulator: Val<I16>, base: Val<I8>) -> AluResult<I16> {
    let high = accumulator.unsigned().shr(8).truncate::<I8>();
    let low = accumulator.truncate::<I8>().add(high.mul(base));
    AluResult {
        result: low.unsigned().extend::<I16>(),
        flags: digit_flags(&low),
    }
}

fn digit_flags(result: &Val<I8>) -> FlagChange {
    FlagChange::partial(
        [StatusFlag::PF, StatusFlag::ZF, StatusFlag::SF]
            .map(|flag| (flag.into(), result_flag(result, flag))),
    )
}
