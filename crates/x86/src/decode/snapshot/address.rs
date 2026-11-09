//! Snapshot address fields use the same register layouts as runtime decoding.

use super::SnapshotCursor;
use crate::{
    address::{AddressSize, EffectiveAddress, IndexTerm, RegisterTerm},
    decode::address16::{BASES, INDICES},
    instruction::FieldWidth,
    register::Gpr32,
    BlockError,
};

pub(super) struct SnapshotAddress {
    size: AddressSize,
    base: Option<Gpr32>,
    index: Option<(Gpr32, u32)>,
    displacement: u32,
}

impl SnapshotAddress {
    pub(super) fn bind(self) -> EffectiveAddress<u32> {
        EffectiveAddress {
            size: self.size,
            base: self.base.map(named),
            index: self.index.map(|(register, shift)| IndexTerm {
                register: named(register),
                shift,
            }),
            displacement: self.displacement,
        }
    }
}

impl SnapshotCursor<'_> {
    pub(super) fn decode_address(
        &mut self,
        modrm: u8,
        size: AddressSize,
    ) -> Result<SnapshotAddress, BlockError> {
        match size {
            AddressSize::Bits16 => self.decode_address16(modrm),
            AddressSize::Bits32 => self.decode_address32(modrm),
        }
    }

    fn decode_address16(&mut self, modrm: u8) -> Result<SnapshotAddress, BlockError> {
        let mode = modrm >> 6;
        let rm = usize::from(modrm & 7);
        let no_base = mode == 0 && rm == 6;
        let displacement = if mode == 2 || no_base {
            self.integer(FieldWidth::Word)?
        } else if mode == 1 {
            self.byte()? as i8 as i32 as u32
        } else {
            0
        };
        Ok(SnapshotAddress {
            size: AddressSize::Bits16,
            base: (!no_base).then(|| BASES[rm]),
            index: INDICES.get(rm).map(|&register| (register, 0)),
            displacement,
        })
    }

    fn decode_address32(&mut self, modrm: u8) -> Result<SnapshotAddress, BlockError> {
        let mode = modrm >> 6;
        let rm = modrm & 7;
        let (base, index) = if rm == 4 {
            let sib = self.byte()?;
            let index = if (sib >> 3) & 7 == 4 {
                None
            } else {
                Some((Gpr32::from_code(sib >> 3), u32::from(sib >> 6)))
            };
            (sib & 7, index)
        } else {
            (rm, None)
        };
        let no_base = mode == 0 && base == 5;
        let displacement = if mode == 2 || no_base {
            self.integer(FieldWidth::Dword)?
        } else if mode == 1 {
            self.byte()? as i8 as i32 as u32
        } else {
            0
        };
        Ok(SnapshotAddress {
            size: AddressSize::Bits32,
            base: (!no_base).then(|| Gpr32::from_code(base)),
            index,
            displacement,
        })
    }
}

fn named(register: Gpr32) -> RegisterTerm {
    RegisterTerm {
        register: register.into(),
        present: None,
    }
}
