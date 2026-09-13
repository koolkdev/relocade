//! Runtime address fields for 16-bit ModRM and 32-bit ModRM/SIB layouts.
//! Register values remain part of shared instruction execution.

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::{
    address::{AddressSize, EffectiveAddress, IndexTerm, RegisterTerm},
    decode::address16::{BASES, INDICES},
    instruction::FieldWidth,
    register::Register,
};

use super::cursor::RuntimeCursor;

/// Continues inside the selected layout with its address and consumed cursor.
/// Only the 32-bit r/m=100 layout reads a SIB byte.
pub(super) fn decode<'memory>(
    mut body: FunctionBuilder<'_>,
    cursor: RuntimeCursor<'memory>,
    modrm: &Val<I8>,
    size: AddressSize,
    complete: impl Fn(
        FunctionBuilder<'_>,
        RuntimeCursor<'memory>,
        EffectiveAddress<Val<I32>>,
    ) -> Result<(), BuildError>,
) -> Result<(), BuildError> {
    let mode = modrm.unsigned().shr(6);
    let rm = modrm.and(7).unsigned().extend::<I32>();
    if size == AddressSize::Bits16 {
        let no_base = mode.eq(0).and(rm.eq(6));
        let mut cursor = cursor;
        let displacement = cursor.displacement(&mut body, &mode, &no_base, FieldWidth::Word)?;
        // Packed register codes select a value without duplicating execution paths.
        let packed = |registers: &[crate::register::Gpr32]| {
            registers
                .iter()
                .enumerate()
                .fold(0u32, |bits, (index, register)| {
                    bits | ((*register as u32) << (index * 4))
                })
        };
        let base = body
            .value::<I32>(packed(&BASES))?
            .unsigned()
            .shr(rm.shl(2))
            .and(7);
        let index = body
            .value::<I32>(packed(&INDICES))?
            .unsigned()
            .shr(rm.shl(2))
            .and(7);
        return complete(
            body,
            cursor,
            EffectiveAddress {
                size,
                base: Some(RegisterTerm {
                    register: Register::indexed(base),
                    present: Some(no_base.eq(0)),
                }),
                index: Some(IndexTerm {
                    register: RegisterTerm {
                        register: Register::indexed(index),
                        present: Some(rm.unsigned().lt(4)),
                    },
                    shift: 0.into(),
                }),
                displacement,
            },
        );
    }
    let complete_layout = |mut body: FunctionBuilder<'_>,
                           mut cursor: RuntimeCursor<'memory>,
                           sib: Option<Val<I8>>| {
        let base = match &sib {
            Some(sib) => sib.and(7).unsigned().extend::<I32>(),
            None => rm.clone(),
        };
        let no_base = mode.eq(0).and(base.eq(5));
        let displacement = cursor.displacement(&mut body, &mode, &no_base, FieldWidth::Dword)?;
        let index = sib.map(|sib| IndexTerm {
            register: RegisterTerm {
                register: Register::<I32>::indexed(
                    sib.unsigned().shr(3).unsigned().extend::<I32>(),
                ),
                present: Some(sib.unsigned().shr(3).and(7).ne(4)),
            },
            shift: sib.unsigned().shr(6).unsigned().extend::<I32>(),
        });
        let address = EffectiveAddress {
            size,
            base: Some(RegisterTerm {
                register: Register::<I32>::indexed(base),
                present: Some(no_base.eq(0)),
            }),
            index,
            displacement,
        };
        complete(body, cursor, address)
    };
    body.if_else(
        rm.eq(4),
        |mut arm| {
            let mut sib_cursor = cursor.clone();
            let sib = sib_cursor.byte(&mut arm)?;
            complete_layout(arm, sib_cursor, Some(sib))
        },
        |arm| complete_layout(arm, cursor.clone(), None),
    )?;
    body.trap()
}
