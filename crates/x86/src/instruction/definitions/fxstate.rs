//! FXSR transfers share x87 images and the retained XMM/MXCSR state owners.

use super::*;
use crate::{
    address::MemoryAddress, execution::OperandSpan, memory::Intent, register::RegisterCode,
    x87::ExtendedBits, StoredSimd,
};
use wasm86_compiler::{I64, V128};

instruction_families! {
    FXSAVE {
        execute: save;
        forms { NP 0x0f 0xae / 0 => operands(mem); }
    }
    FXRSTOR {
        execute: restore;
        forms { NP 0x0f 0xae / 1 => operands(mem); }
    }
}

fn restore(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError> {
    let operand =
        execution.memory_operand(source, OperandSpan::aligned(512, 16), Intent::Read, &[])?;
    let mut environment = std::array::from_fn(|_| Val::<I32>::from(0));
    for (index, word) in environment.iter_mut().enumerate() {
        *word = operand.read::<I32>(execution, index as u32 * 4)?;
    }
    let mxcsr = operand.read::<I32>(execution, 24)?;
    let mut registers = std::array::from_fn(|_| ExtendedBits {
        significand: 0.into(),
        sign_exponent: 0.into(),
    });
    for (index, register) in registers.iter_mut().enumerate() {
        let offset = 32 + index as u32 * 16;
        *register = ExtendedBits {
            significand: operand.read::<I64>(execution, offset)?,
            sign_exponent: operand.read::<I16>(execution, offset + 8)?,
        };
    }
    let mut xmm: [Val<V128>; 8] = std::array::from_fn(|_| 0_u128.into());
    for (index, value) in xmm.iter_mut().enumerate() {
        *value = operand.read::<V128>(execution, 160 + index as u32 * 16)?;
    }

    // Reserved MXCSR bits fault before any restored state is installed. The
    // saved MXCSR_MASK is informational and cannot relax this validation.
    execution.load_mxcsr(mxcsr)?;
    let pointer_size = execution.profile().code_default_size();
    execution
        .x87()
        .restore_fx(pointer_size, &environment, &registers)?;
    for (index, value) in xmm.into_iter().enumerate() {
        execution.write_xmm(RegisterCode::from_code(index as u8), value)?;
    }
    Ok(())
}

fn save(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError> {
    let operand = execution.memory_operand(
        destination,
        OperandSpan::aligned(512, 16),
        Intent::Write,
        &[],
    )?;
    let registers = execution.x87().stack_bits()?;
    let pointer_size = execution.profile().code_default_size();
    let environment = execution.x87().fx_environment(pointer_size)?;
    for (index, word) in environment.iter().enumerate() {
        operand.write(execution, index as u32 * 4, word)?;
    }
    let mxcsr = execution.read_mxcsr()?.bits();
    operand.write(execution, 24, mxcsr)?;
    operand.write::<I32>(execution, 28, StoredSimd::MXCSR_MASK)?;
    for (index, register) in registers.iter().enumerate() {
        let offset = 32 + index as u32 * 16;
        operand.write(execution, offset, &register.significand)?;
        operand.write(execution, offset + 8, &register.sign_exponent)?;
    }
    for index in 0..8 {
        let xmm = execution.read_xmm(RegisterCode::from_code(index))?;
        operand.write(execution, 160 + u32::from(index) * 16, xmm)?;
    }
    Ok(())
}
