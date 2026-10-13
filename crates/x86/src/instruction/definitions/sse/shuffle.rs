//! Integer shuffles replace the destination from an aligned register/m128 source.

use super::*;
use crate::alu::{shuffle_dwords, shuffle_words};

instruction_families! {
    PSHUFD {
        execute: shuffle(|input, control| shuffle_dwords(input.clone(), input, control));
        forms { P66 0x0f 0x70 => operands(xmm, xmm_rm, imm8); }
    }
    PSHUFLW {
        execute: shuffle(|input, control| shuffle_words(input, control, false));
        forms { PF2 0x0f 0x70 => operands(xmm, xmm_rm, imm8); }
    }
    PSHUFHW {
        execute: shuffle(|input, control| shuffle_words(input, control, true));
        forms { PF3 0x0f 0x70 => operands(xmm, xmm_rm, imm8); }
    }
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
