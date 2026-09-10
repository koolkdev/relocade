//! Pure x86 operand results and their status-flag changes.
//! Operand access belongs to instruction execution; flag storage belongs to state.

mod arithmetic;
pub(crate) mod flags;
mod logic;
mod rotates;
mod shifts;
mod unary;

pub(crate) use arithmetic::ArithmeticOp;
pub(crate) use logic::LogicOp;
pub(crate) use rotates::RotateDirection;
pub(crate) use shifts::{DoubleShiftOp, ShiftOp};
pub(crate) use unary::UnaryOp;

use wasm86_compiler::{MemoryInt, Val, I1};

use flags::{FlagChange, StatusFlag};

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
