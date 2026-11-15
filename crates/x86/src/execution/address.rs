//! Implicit memory operands and registers follow the instruction's address size.

use super::ExecutionBuilder;
use crate::{
    address::{EffectiveAddress, MemoryAddress, RegisterTerm},
    instruction::{Location, TypedLocation},
    register::{Gpr32, Register, RegisterType},
    segment::{Segment, SegmentSelection},
};
use relocade_compiler::{BuildError, Val, I16, I32};

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn wrap_address(&self, value: Val<I32>) -> Val<I32> {
        value.and(&self.address_mask)
    }

    /// Implicit data sources use DS unless the current instruction overrides it.
    pub(crate) fn data_segment(&self) -> SegmentSelection {
        self.segment_override.apply(&Segment::Ds.into())
    }

    /// Defers a register-based memory access using the current address size.
    /// The caller selects the segment, including whether an override applies.
    pub(crate) fn memory_at_register<T: RegisterType>(
        &self,
        base: Gpr32,
        segment: SegmentSelection,
    ) -> TypedLocation<T> {
        TypedLocation::new(Location::Memory(
            self.memory_address_at_register(base, segment).into(),
        ))
    }

    /// Constructs an implicit address independently of the transferred value's width.
    pub(crate) fn memory_address_at_register(
        &self,
        base: Gpr32,
        segment: SegmentSelection,
    ) -> MemoryAddress<Val<I32>> {
        MemoryAddress {
            segment,
            offset: EffectiveAddress {
                mask: self.address_mask.clone(),
                base: Some(RegisterTerm {
                    register: base.into(),
                    present: None,
                }),
                index: None,
                displacement: 0.into(),
            },
        }
    }

    pub(crate) fn read_address_register(
        &mut self,
        register: Gpr32,
    ) -> Result<Val<I32>, BuildError> {
        match self.body.constant_bits(&self.address_mask)? {
            Some(mask) if mask == u64::from(u16::MAX) => Ok(self
                .state
                .read_register::<I16>(&mut self.body, Register::named(register))?
                .unsigned()
                .extend::<I32>()),
            _ => {
                let value = self.state.read_register(&mut self.body, register)?;
                Ok(self.wrap_address(value))
            }
        }
    }

    pub(crate) fn write_address_register(
        &mut self,
        register: Gpr32,
        value: Val<I32>,
    ) -> Result<(), BuildError> {
        match self.body.constant_bits(&self.address_mask)? {
            Some(mask) if mask == u64::from(u16::MAX) => self.state.write_register(
                &mut self.body,
                Register::named(register),
                value.truncate::<I16>(),
            ),
            Some(mask) if mask == u64::from(u32::MAX) => {
                self.state.write_register(&mut self.body, register, value)
            }
            _ => {
                let previous: Val<I32> = self.state.read_register(&mut self.body, register)?;
                let updated = previous
                    .and(self.address_mask.xor(u32::MAX))
                    .or(self.wrap_address(value));
                self.state.write_register(&mut self.body, register, updated)
            }
        }
    }
}
