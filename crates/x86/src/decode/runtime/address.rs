//! Runtime address fields for 16-bit ModRM and 32-bit ModRM/SIB layouts.
//! Register values remain part of shared instruction execution.

use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I32, I8};

use crate::{
    address::{EffectiveAddress, IndexTerm, RegisterTerm},
    decode::address16::{BASES, INDICES},
    register::{Gpr32, Register},
};

use super::cursor::RuntimeCursor;

/// Reads the selected layout's bytes and returns one deferred address.
pub(super) fn decode(
    body: &mut BlockBuilder<'_>,
    cursor: &mut RuntimeCursor<'_>,
    modrm: &Val<I8>,
    dword: &Val<I1>,
) -> Result<EffectiveAddress<Val<I32>>, BuildError> {
    let mode = modrm.unsigned().shr(6);
    let rm = modrm.and(7).unsigned().extend::<I32>();

    // In 16-bit addressing r/m=100 names SI; only the 32-bit layout reads SIB.
    let has_sib = dword.and(rm.eq(4));
    let sib = cursor.byte_if(body, &has_sib)?;
    let sib_base = sib.and(7).unsigned().extend::<I32>();
    let sib_index = sib.unsigned().shr(3).and(7).unsigned().extend::<I32>();
    let base32 = has_sib.select(sib_base, &rm);

    // With mod=00, r/m=110 omits the base in 16-bit addressing;
    // base=101 (including SIB.base) does so in 32-bit addressing.
    let no_base = mode.eq(0).and(dword.select(base32.eq(5), rm.eq(6)));
    let displacement = cursor.displacement(body, &mode, &no_base, dword)?;

    let base = dword.select(base32, register_from_table(&BASES, &rm));
    let index = dword.select(&sib_index, register_from_table(&INDICES, &rm));
    // Only the first four 16-bit layouts have two registers; SIB.index=100
    // suppresses the index in the 32-bit layout.
    let has_index = dword.select(has_sib.and(sib_index.ne(4)), rm.unsigned().lt(4));
    Ok(EffectiveAddress {
        mask: dword.select(u32::MAX, u32::from(u16::MAX)),
        base: Some(RegisterTerm {
            register: Register::indexed(base),
            present: Some(no_base.eq(false)),
        }),
        index: Some(IndexTerm {
            register: RegisterTerm {
                register: Register::indexed(index),
                present: Some(has_index),
            },
            // An absent SIB reads as zero, giving the 16-bit layouts no scaling.
            shift: sib.unsigned().shr(6).unsigned().extend::<I32>(),
        }),
        displacement,
    })
}

fn register_from_table(registers: &[Gpr32], rm: &Val<I32>) -> Val<I32> {
    // Four bits per r/m value hold a parent register code.
    let packed = registers
        .iter()
        .enumerate()
        .fold(0u32, |bits, (index, register)| {
            bits | ((*register as u32) << (index * 4))
        });
    Val::<I32>::from(packed).unsigned().shr(rm.shl(2)).and(7)
}
