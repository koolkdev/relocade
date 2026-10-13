//! MMX transfers and x87 tag reset.

use super::*;

instruction_families! {
    PMOVMSKB {
        execute: byte_mask;
        forms { NP 0x0f 0xd7 => dword(modrm_reg, register(mm_rm)); }
    }
    MOVD_LOAD {
        execute: load_doubleword;
        forms { NP 0x0f 0x6e => dword(mm, rm); }
    }
    MOVD_STORE {
        execute: store_doubleword;
        forms { NP 0x0f 0x7e => dword(rm, mm); }
    }
    MOVQ {
        execute: move_quadword;
        forms {
            NP 0x0f 0x6f => operands(mm, mm_rm);
            NP 0x0f 0x7f => operands(mm_rm, mm);
        }
    }
    MOVQ2DQ {
        execute: move_to_xmm;
        forms { PF3 0x0f 0xd6 => operands(xmm, register(mm_rm)); }
    }
    MOVDQ2Q {
        execute: move_from_xmm;
        forms { PF2 0x0f 0xd6 => operands(mm, register(xmm_rm)); }
    }
    EMMS {
        execute: empty;
        forms { NP 0x0f 0x77 => no_operands(); }
    }
}

fn byte_mask(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I32>,
    source: RegisterCode,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let bits = execution.read_mmx(source)?;
        let mask = Val::<V64>::from_bits(bits).bitmask::<I8>();
        destination.write(execution, mask)
    })
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

fn empty(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.empty_mmx()
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
