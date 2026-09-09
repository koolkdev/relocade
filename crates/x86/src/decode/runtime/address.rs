//! Runtime address-field decoding for ordinary and SIB memory layouts.
//! Register values remain part of shared instruction execution.

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::{
    address::{Address32, IndexTerm, RegisterTerm},
    register::Register,
};

use super::cursor::RuntimeCursor;

/// Continues inside the selected layout with its address and consumed cursor.
/// Ordinary addresses have no index term and never read a SIB byte.
pub(super) fn decode<'memory>(
    mut body: FunctionBuilder<'_>,
    cursor: RuntimeCursor<'memory>,
    modrm: &Val<I8>,
    complete: impl Fn(
        FunctionBuilder<'_>,
        RuntimeCursor<'memory>,
        Address32<Val<I32>>,
    ) -> Result<(), BuildError>,
) -> Result<(), BuildError> {
    let mode = modrm.unsigned().shr(6);
    let rm = modrm.and(7).unsigned().extend::<I32>();
    let complete_layout = |mut body: FunctionBuilder<'_>,
                           mut cursor: RuntimeCursor<'memory>,
                           sib: Option<Val<I8>>| {
        let base = match &sib {
            Some(sib) => sib.and(7).unsigned().extend::<I32>(),
            None => rm.clone(),
        };
        let no_base = mode.eq(0).and(base.eq(5));
        let displacement = cursor.displacement(&mut body, &mode, &no_base)?;
        let index = sib.map(|sib| IndexTerm {
            register: RegisterTerm {
                register: Register::<I32>::indexed(
                    sib.unsigned().shr(3).unsigned().extend::<I32>(),
                ),
                present: Some(sib.unsigned().shr(3).and(7).ne(4)),
            },
            shift: sib.unsigned().shr(6).unsigned().extend::<I32>(),
        });
        let address = Address32 {
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
