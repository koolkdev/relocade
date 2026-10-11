//! MMX is an integer view of the physical x87 slots, independent of TOP.

use wasm86_compiler::{BuildError, Val, I64};

use crate::{
    register::RegisterCode,
    x87::{ExtendedBits, ExtendedValue},
};

use super::X87Access;

impl X87Access<'_, '_> {
    pub(crate) fn read_mmx(&mut self, register: RegisterCode) -> Result<Val<I64>, BuildError> {
        let slot = self.state.registers.physical(self.body, register)?;
        Ok(self
            .state
            .registers
            .read(self.body, &slot)?
            .bits()
            .significand)
    }

    pub(crate) fn write_mmx(
        &mut self,
        register: RegisterCode,
        value: Val<I64>,
    ) -> Result<(), BuildError> {
        let slot = self.state.registers.physical(self.body, register)?;
        self.state.registers.write_value(
            self.body,
            &slot,
            &ExtendedValue::from_bits(ExtendedBits {
                significand: value,
                sign_exponent: 0xffff_u32.into(),
            }),
            &true.into(),
        )
    }

    pub(crate) fn complete_mmx(&mut self) -> Result<(), BuildError> {
        self.mmx_tags(0)
    }

    pub(crate) fn empty_mmx(&mut self) -> Result<(), BuildError> {
        self.mmx_tags(0xffff)
    }

    fn mmx_tags(&mut self, tags: u16) -> Result<(), BuildError> {
        // Intel SDM Vol. 3A, Table 15-2 includes EMMS in the TOP reset.
        self.state.registers.reset(self.body, tags)
    }
}
