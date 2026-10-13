//! Sign masks copy lane sign bits to a zero-extended general-purpose register.

use super::*;
use wasm86_compiler::I64;

instruction_families! {
    PMOVMSKB {
        execute: mask(|vector| vector.bitmask::<I8>());
        forms { P66 0x0f 0xd7 => dword(modrm_reg, register(xmm_rm)); }
    }
    MOVMSKPS {
        execute: mask(|vector| vector.bitmask::<I32>());
        forms { NP 0x0f 0x50 => dword(modrm_reg, register(xmm_rm)); }
    }
    MOVMSKPD {
        execute: mask(|vector| vector.bitmask::<I64>());
        forms { P66 0x0f 0x50 => dword(modrm_reg, register(xmm_rm)); }
    }
}

fn mask(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I32>,
    source: RegisterCode,
    operation: impl FnOnce(Val<V128>) -> Val<I32>,
) -> Result<(), BuildError> {
    let mask = operation(execution.read_xmm(source)?);
    destination.write(execution, mask)
}
