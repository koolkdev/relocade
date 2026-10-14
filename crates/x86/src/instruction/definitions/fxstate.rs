//! FXSR transfers share x87 images and the retained XMM/MXCSR state owners.

use super::*;
use crate::{
    address::MemoryAddress, execution::OperandSpan, memory::Intent, register::RegisterCode,
    StoredSimd,
};

instruction_families! {
    FXSAVE {
        execute: save;
        forms { NP 0x0f 0xae / 0 => operands(mem); }
    }
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
