//! Byte sign masks are copied to a zero-extended general-purpose register.

use super::*;

instruction_families! {
    PMOVMSKB {
        execute: byte_mask;
        requires: [Sse];
        forms { NP 0x0f 0xd7 => dword(modrm_reg, register(mm_rm)); }
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
