//! Bitwise operand results and their status-flag rules.

use wasm86_compiler::{MemoryInt, Val, I1};

use super::{
    flags::{AnyFlagSource, FlagChange, FlagSource, StatusFlag},
    result_flag, AluResult,
};

#[derive(Clone, Copy)]
pub(crate) enum LogicOp {
    And,
    Or,
    Xor,
}

impl LogicOp {
    pub(crate) fn apply<T: MemoryInt>(self, left: Val<T>, right: Val<T>) -> AluResult<T>
    where
        FlagSource<T>: Into<AnyFlagSource>,
    {
        let result = match self {
            Self::And => left.and(right),
            Self::Or => left.or(right),
            Self::Xor => left.xor(right),
        };
        let flags = FlagSource::Logic {
            result: result.clone(),
        };
        AluResult {
            result,
            flags: FlagChange::from(flags),
        }
    }
}

pub(super) fn flag<T: MemoryInt>(result: &Val<T>, flag: StatusFlag) -> Val<I1> {
    match flag {
        StatusFlag::CF | StatusFlag::OF => false.into(),
        // AF is architecturally undefined. Zero is our deterministic policy;
        // preserving it would require evaluating the previous flag source.
        StatusFlag::AF => false.into(),
        StatusFlag::PF | StatusFlag::ZF | StatusFlag::SF => result_flag(result, flag),
    }
}
