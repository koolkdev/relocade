//! Word transfers mask the immediate index and preserve all other vector lanes.

use super::*;

instruction_families! {
    PINSRW {
        execute: insert;
        forms { P66 0x0f 0xc4 => operands(xmm, rm16, imm8); }
    }
    PEXTRW {
        execute: extract;
        forms { P66 0x0f 0xc5 => dword(modrm_reg, register(xmm_rm), imm8); }
    }
}

fn insert(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: TypedLocation<I16>,
    index: Input<I8>,
) -> Result<(), BuildError> {
    let word = source.read(execution)?;
    let index = index.read(execution)?.unsigned().extend::<I32>();
    let vector = execution.read_xmm(destination.clone())?;
    execution.write_xmm(destination, vector.replace_lane(index, word))
}

fn extract(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I32>,
    source: RegisterCode,
    index: Input<I8>,
) -> Result<(), BuildError> {
    let index = index.read(execution)?.unsigned().extend::<I32>();
    let word = execution.read_xmm(source)?.extract_lane::<I16>(index);
    destination.write(execution, word.unsigned().extend::<I32>())
}
