//! Whole-register and scalar moves between MMX, XMM, GPRs and memory.

use super::*;
use crate::{address::MemoryAddress, memory::Intent, register::Gpr32};

instruction_families! {
    MOVD_LOAD {
        execute: load_doubleword;
        requires: [Mmx];
        forms { NP 0x0f 0x6e => dword(mm, rm); }
    }
    MOVD_STORE {
        execute: store_doubleword;
        requires: [Mmx];
        forms { NP 0x0f 0x7e => dword(rm, mm); }
    }
    MOVQ {
        execute: move_quadword;
        requires: [Mmx];
        forms {
            NP 0x0f 0x6f => operands(mm, mm_rm);
            NP 0x0f 0x7f => operands(mm_rm, mm);
        }
    }
    MOVQ2DQ {
        execute: move_to_xmm;
        requires: [Sse2];
        forms { PF3 0x0f 0xd6 => operands(xmm, register(mm_rm)); }
    }
    MOVDQ2Q {
        execute: move_from_xmm;
        requires: [Sse2];
        forms { PF2 0x0f 0xd6 => operands(mm, register(xmm_rm)); }
    }
    MOVNTQ {
        execute: store_non_temporal;
        requires: [Sse];
        forms { NP 0x0f 0xe7 => operands(mem, mm); }
    }
    MASKMOVQ {
        execute: store_masked;
        requires: [Sse];
        forms { NP 0x0f 0xf7 => operands(mm, register(mm_rm)); }
    }
}

fn load_doubleword(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: TypedLocation<I32>,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let value = source.read(execution)?.unsigned().extend::<I64>();
        execution.write_mmx(destination, value)
    })
}

fn store_doubleword(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I32>,
    source: RegisterCode,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let value = execution.read_mmx(source)?.truncate::<I32>();
        destination.write(execution, value)
    })
}

fn move_quadword(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MmxLocation,
    source: MmxLocation,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let value = source.read::<I64>(execution)?;
        destination.write(execution, value)
    })
}

fn move_to_xmm(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: RegisterCode,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let bits = execution.read_mmx(source)?;
        execution.write_xmm(destination, Val::<V64>::from_bits(bits).zero_extend())
    })
}

fn move_from_xmm(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: RegisterCode,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let bits = execution.read_xmm(source)?.low_half().to_bits();
        execution.write_mmx(destination, bits)
    })
}

fn store_non_temporal(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MemoryAddress<Val<I32>>,
    source: RegisterCode,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let value = execution.read_mmx(source)?;
        execution
            .memory_operand(destination, 8, Intent::Write, &[])?
            .write(execution, 0, value)
    })
}

fn store_masked(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: RegisterCode,
    mask: RegisterCode,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let value = Val::<V64>::from_bits(execution.read_mmx(source)?);
        let mask = Val::<V64>::from_bits(execution.read_mmx(mask)?);
        let address = execution.memory_address_at_register(Gpr32::Edi, execution.data_segment());
        execution.write_masked_memory(address, value, mask)
    })
}
