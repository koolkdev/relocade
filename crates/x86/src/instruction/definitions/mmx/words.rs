//! MMX word transfers share vector lane selection and retain x87 admission.

use super::*;

instruction_families! {
    PINSRW {
        execute: insert;
        forms { NP 0x0f 0xc4 => operands(mm, rm16, imm8); }
    }
    PEXTRW {
        execute: extract;
        forms { NP 0x0f 0xc5 => dword(modrm_reg, register(mm_rm), imm8); }
    }
}

fn insert(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: TypedLocation<I16>,
    index: Input<I8>,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let word = source.read(execution)?;
        let index = index.read(execution)?.unsigned().extend::<I32>();
        let bits = execution.read_mmx(destination.clone())?;
        let vector = Val::<V64>::from_bits(bits);
        execution.write_mmx(destination, vector.replace_lane(index, word).to_bits())
    })
}

fn extract(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I32>,
    source: RegisterCode,
    index: Input<I8>,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let index = index.read(execution)?.unsigned().extend::<I32>();
        let bits = execution.read_mmx(source)?;
        let vector = Val::<V64>::from_bits(bits);
        destination.write(
            execution,
            vector.extract_lane::<I16>(index).unsigned().extend::<I32>(),
        )
    })
}
