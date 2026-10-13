//! PSHUFW permutes the four words of a register or unaligned m64 source.

use super::*;
use crate::simd::shuffle_words;

instruction_families! {
    PSHUFW {
        execute: shuffle;
        forms { NP 0x0f 0x70 => operands(mm, mm_rm, imm8); }
    }
}

fn shuffle(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: MmxLocation,
    control: Input<I8>,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let input = source.read::<I64>(execution)?;
        let control = control.read(execution)?;
        let vector = Val::<V64>::from_bits(input);
        execution.write_mmx(destination, shuffle_words(vector, control, false).to_bits())
    })
}
