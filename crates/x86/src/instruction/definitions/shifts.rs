use super::*;
use crate::{
    alu::{
        flags::{AnyFlagSource, FlagSource},
        DoubleShiftOp, RotateDirection, ShiftOp,
    },
    register::RegisterType,
};

instruction_families! {
    ROL {
        execute: rotate(RotateDirection::Left);
        forms {
            0xD0 /0 => byte(rm, constant(1));
            0xD1 /0 => word_or_dword(rm, constant(1));
            0xD2 /0 => byte(rm, CL);
            0xD3 /0 => word_or_dword(rm, CL);
            0xC0 /0 => byte(rm, imm8);
            0xC1 /0 => word_or_dword(rm, imm8);
        }
    }
    ROR {
        execute: rotate(RotateDirection::Right);
        forms {
            0xD0 /1 => byte(rm, constant(1));
            0xD1 /1 => word_or_dword(rm, constant(1));
            0xD2 /1 => byte(rm, CL);
            0xD3 /1 => word_or_dword(rm, CL);
            0xC0 /1 => byte(rm, imm8);
            0xC1 /1 => word_or_dword(rm, imm8);
        }
    }
    RCL {
        execute: rotate_through_carry(RotateDirection::Left);
        forms {
            0xD0 /2 => byte(rm, constant(1));
            0xD1 /2 => word_or_dword(rm, constant(1));
            0xD2 /2 => byte(rm, CL);
            0xD3 /2 => word_or_dword(rm, CL);
            0xC0 /2 => byte(rm, imm8);
            0xC1 /2 => word_or_dword(rm, imm8);
        }
    }
    RCR {
        execute: rotate_through_carry(RotateDirection::Right);
        forms {
            0xD0 /3 => byte(rm, constant(1));
            0xD1 /3 => word_or_dword(rm, constant(1));
            0xD2 /3 => byte(rm, CL);
            0xD3 /3 => word_or_dword(rm, CL);
            0xC0 /3 => byte(rm, imm8);
            0xC1 /3 => word_or_dword(rm, imm8);
        }
    }
    SHL {
        execute: shift(ShiftOp::Left);
        forms {
            0xD0 /4 => byte(rm, constant(1));
            0xD1 /4 => word_or_dword(rm, constant(1));
            0xD2 /4 => byte(rm, CL);
            0xD3 /4 => word_or_dword(rm, CL);
            0xC0 /4 => byte(rm, imm8);
            0xC1 /4 => word_or_dword(rm, imm8);
        }
    }
    SHR {
        execute: shift(ShiftOp::RightLogical);
        forms {
            0xD0 /5 => byte(rm, constant(1));
            0xD1 /5 => word_or_dword(rm, constant(1));
            0xD2 /5 => byte(rm, CL);
            0xD3 /5 => word_or_dword(rm, CL);
            0xC0 /5 => byte(rm, imm8);
            0xC1 /5 => word_or_dword(rm, imm8);
        }
    }
    SAR {
        execute: shift(ShiftOp::RightArithmetic);
        forms {
            0xD0 /7 => byte(rm, constant(1));
            0xD1 /7 => word_or_dword(rm, constant(1));
            0xD2 /7 => byte(rm, CL);
            0xD3 /7 => word_or_dword(rm, CL);
            0xC0 /7 => byte(rm, imm8);
            0xC1 /7 => word_or_dword(rm, imm8);
        }
    }
    SHLD {
        execute: double_shift(DoubleShiftOp::Left);
        forms {
            0x0F 0xA4 => word_or_dword(rm, modrm_reg, imm8);
            0x0F 0xA5 => word_or_dword(rm, modrm_reg, CL);
        }
    }
    SHRD {
        execute: double_shift(DoubleShiftOp::Right);
        forms {
            0x0F 0xAC => word_or_dword(rm, modrm_reg, imm8);
            0x0F 0xAD => word_or_dword(rm, modrm_reg, CL);
        }
    }
}

fn rotate<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    count: Input<I8>,
    direction: RotateDirection,
) -> Result<(), BuildError> {
    destination.update(execution, |execution, input| {
        let count = count.read(execution)?.and(31).unsigned().extend::<I32>();
        let outcome = direction.rotate(input, count);
        execution.set_flags(outcome.flags)?;
        Ok(outcome.result)
    })
}

fn rotate_through_carry<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    count: Input<I8>,
    direction: RotateDirection,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    destination.update(execution, |execution, input| {
        let count = count.read(execution)?.and(31).unsigned().extend::<I32>();
        let carry = execution.condition(Condition::B)?;
        let outcome = direction.rotate_through_carry(input, count, carry);
        execution.set_flags(outcome.flags)?;
        Ok(outcome.result)
    })
}

fn shift<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    count: Input<I8>,
    operation: ShiftOp,
) -> Result<(), BuildError>
where
    FlagSource<T>: Into<AnyFlagSource>,
{
    destination.update(execution, |execution, input| {
        let count = count.read(execution)?.and(31).unsigned().extend::<I32>();
        let outcome = operation.apply(input, count);
        execution.set_flags(outcome.flags)?;
        Ok(outcome.result)
    })
}

fn double_shift<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<T>,
    count: Input<I8>,
    operation: DoubleShiftOp,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    FlagSource<T>: Into<AnyFlagSource>,
{
    destination.update(execution, |execution, input| {
        let source = source.read(execution)?;
        let count = count.read(execution)?.and(31).unsigned().extend::<I32>();
        let outcome = operation.apply(input, source, count);
        execution.set_flags(outcome.flags)?;
        Ok(outcome.result)
    })
}
