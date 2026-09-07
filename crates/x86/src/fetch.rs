//! Checked instruction reads for an entry with no earlier completed instructions.
//! Faults return directly, without publishing CPU state.

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::{
    memory::{Intent, Memory},
    state::exit,
};

pub(super) fn byte(
    body: &mut FunctionBuilder<'_>,
    memory: Memory,
    address: &Val<I32>,
) -> Result<Val<I8>, BuildError> {
    let access = memory.resolve_access::<I8>(body, address, Intent::Fetch)?;
    body.if_(&access.fault.condition, |arm| {
        arm.return_(exit::page_fault(&access.fault.address, &access.fault.error))
    })?;
    memory.read(body, &access)
}

pub(super) fn dword(
    body: &mut FunctionBuilder<'_>,
    memory: Memory,
    start: &Val<I32>,
) -> Result<Val<I32>, BuildError> {
    let direct = memory.check_direct_access(body, start, 4, Intent::Fetch)?;
    body.if_value::<I32>(
        &direct.unavailable,
        |mut arm| {
            // A failed range proof must be retried in byte order to identify the
            // first unavailable instruction address, including across EIP wrap.
            let mut value = byte(&mut arm, memory, start)?.unsigned().extend::<I32>();
            for offset in 1..4 {
                let next = byte(&mut arm, memory, &start.add(offset))?;
                value = value.or(next.unsigned().extend::<I32>().shl(offset * 8));
            }
            arm.yield_(value)
        },
        |mut arm| {
            let value = memory.load::<I32>(&mut arm, &direct.physical, 0)?;
            arm.yield_(value)
        },
    )
}
