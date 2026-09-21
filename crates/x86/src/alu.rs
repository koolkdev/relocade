//! Pure x86 operand results, status-flag changes and numeric fault conditions.
//! Operand access belongs to instruction execution; flag storage belongs to state.

mod adjust;
mod arithmetic;
mod bit_scans;
mod bit_tests;
mod divide;
mod logic;
mod multiply;
mod rotates;
mod shifts;
mod status;
mod unary;

pub(crate) use adjust::{adjust_after_multiply, adjust_before_division};
pub(crate) use arithmetic::ArithmeticOp;
pub(crate) use bit_scans::BitScanOp;
pub(crate) use bit_tests::BitTestOp;
pub(crate) use divide::DivideOp;
pub(crate) use logic::LogicOp;
pub(crate) use multiply::MultiplyOp;
pub(crate) use rotates::RotateDirection;
pub(crate) use shifts::{DoubleShiftOp, ShiftOp};
pub(crate) use status::{AnyStatusSource, StatusSource};
pub(crate) use unary::UnaryOp;

use wasm86_compiler::{AtLeast, MemoryInt, Val, I1, I16, I32, I64, I8};

use crate::flags::{FlagChange, StatusFlag};

/// Full products and division dividends have twice the operand's logical width.
pub(crate) trait DoubleWidth: MemoryInt {
    type Double: MemoryInt + AtLeast<Self> + AtLeast<I16>;
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

pub(crate) struct AluResult<T: MemoryInt> {
    pub(crate) result: Val<T>,
    pub(crate) flags: FlagChange,
}

fn result_flag<T: MemoryInt>(result: &Val<T>, flag: StatusFlag) -> Val<I1> {
    match flag {
        StatusFlag::PF => result.and(0xff).popcnt().and(1).eq(0),
        StatusFlag::ZF => result.eq(0),
        StatusFlag::SF => bit(result, T::BYTES * 8 - 1),
        _ => unreachable!("only parity, zero and sign depend on the result alone"),
    }
}

fn bit<T: MemoryInt>(value: &Val<T>, index: u32) -> Val<I1> {
    value.unsigned().shr(index).truncate::<I1>()
}
