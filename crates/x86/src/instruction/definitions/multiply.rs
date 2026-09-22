use super::*;
use crate::alu::{AnyStatusSource, DoubleWidth, MultiplyOp, StatusSource};
use crate::register::{Gpr32, RegisterType};

instruction_families! {
    MUL {
        execute: implicit_multiply(MultiplyOp::Unsigned);
        forms {
            0xF6 /4 => byte(rm);
            0xF7 /4 => word_or_dword(rm);
        }
    }
    IMUL_IMPLICIT {
        execute: implicit_multiply(MultiplyOp::Signed);
        forms {
            0xF6 /5 => byte(rm);
            0xF7 /5 => word_or_dword(rm);
        }
    }
    IMUL_DESTINATION {
        execute: multiply_destination;
        forms {
            0x0F 0xAF => word_or_dword(modrm_reg, rm);
        }
    }
    IMUL_IMMEDIATE {
        execute: multiply_sources;
        forms {
            0x69 => word_or_dword(modrm_reg, rm, imm);
            0x6B => word_or_dword(modrm_reg, rm, signed_imm8);
        }
    }
}

fn implicit_multiply<T: RegisterType + DoubleWidth>(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: Input<T>,
    operation: MultiplyOp,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    let source = source.read(execution)?;
    let accumulator = TypedLocation::<T>::register(Gpr32::Eax).read(execution)?;
    let outcome = operation.apply(accumulator, source);
    if T::BYTES == 1 {
        TypedLocation::<I16>::register(Gpr32::Eax)
            .write(execution, outcome.result.truncate::<I16>())?;
    } else {
        execution.write_register_pair::<T>(Gpr32::Edx, Gpr32::Eax, outcome.result)?;
    }
    execution.write_flags(outcome.flags)
}

fn multiply_destination<T: RegisterType + DoubleWidth>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    let source = source.read(execution)?;
    destination.update(execution, |execution, previous| {
        let outcome = MultiplyOp::Signed.apply(previous, source);
        execution.write_flags(outcome.flags)?;
        Ok(outcome.result.truncate::<T>())
    })
}

fn multiply_sources<T: RegisterType + DoubleWidth>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    first_source: Input<T>,
    second_source: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    let left = first_source.read(execution)?;
    let right = second_source.read(execution)?;
    let outcome = MultiplyOp::Signed.apply(left, right);
    destination.write(execution, outcome.result.truncate::<T>())?;
    execution.write_flags(outcome.flags)
}
