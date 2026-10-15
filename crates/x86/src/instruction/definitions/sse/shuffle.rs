//! Shuffles select raw lanes from registers or an aligned m128 source.

use super::*;
use crate::simd::{shuffle_dwords, shuffle_qwords, shuffle_words};

instruction_families! {
    SHUFPS {
        execute: shuffle_pair(shuffle_dwords);
        requires: [Sse];
        forms { NP 0x0f 0xc6 => operands(xmm, xmm_rm, imm8); }
    }
    SHUFPD {
        execute: shuffle_pair(shuffle_qwords);
        requires: [Sse2];
        forms { P66 0x0f 0xc6 => operands(xmm, xmm_rm, imm8); }
    }
    PSHUFD {
        execute: shuffle(|input, control| shuffle_dwords(input.clone(), input, control));
        requires: [Sse2];
        forms { P66 0x0f 0x70 => operands(xmm, xmm_rm, imm8); }
    }
    PSHUFLW {
        execute: shuffle(|input, control| shuffle_words(input, control, false));
        requires: [Sse2];
        forms { PF2 0x0f 0x70 => operands(xmm, xmm_rm, imm8); }
    }
    PSHUFHW {
        execute: shuffle(|input, control| shuffle_words(input, control, true));
        requires: [Sse2];
        forms { PF3 0x0f 0x70 => operands(xmm, xmm_rm, imm8); }
    }
}

fn shuffle_pair(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    control: Input<I8>,
    operation: impl FnOnce(Val<V128>, Val<V128>, Val<I8>) -> Val<V128>,
) -> Result<(), BuildError> {
    let control = control.read(execution)?;
    binary(execution, destination, source, |left, right| {
        operation(left, right, control)
    })
}

fn shuffle(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    control: Input<I8>,
    operation: impl FnOnce(Val<V128>, Val<I8>) -> Val<V128>,
) -> Result<(), BuildError> {
    let input = source.read::<V128>(execution, VectorAlignment::Aligned)?;
    let control = control.read(execution)?;
    execution.write_xmm(destination, operation(input, control))
}
