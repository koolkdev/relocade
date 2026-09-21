//! Implicit memory operands and registers follow the instruction's address size.

use super::ExecutionBuilder;
use crate::{
    address::{AddressSize, EffectiveAddress, MemoryAddress, RegisterTerm},
    instruction::{Location, TypedLocation},
    register::{Gpr32, Register, RegisterType},
    segment::{Segment, SegmentSelection},
};
use wasm86_compiler::{BuildError, Val, I16, I32};

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn address_size(&self) -> AddressSize {
        self.address_size
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
            MemoryAddress {
                segment,
                offset: EffectiveAddress {
                    size: self.address_size,
                    base: Some(RegisterTerm {
                        register: base.into(),
                        present: None,
                    }),
                    index: None,
                    displacement: 0.into(),
                },
            }
            .into(),
        ))
    }

    pub(crate) fn read_address_register(
        &mut self,
        register: Gpr32,
    ) -> Result<Val<I32>, BuildError> {
        match self.address_size {
            AddressSize::Bits16 => Ok(self
                .state
                .read_register::<I16>(&mut self.body, Register::named(register))?
                .unsigned()
                .extend::<I32>()),
            AddressSize::Bits32 => self.state.read_register(&mut self.body, register),
        }
    }

    pub(crate) fn write_address_register(
        &mut self,
        register: Gpr32,
        value: Val<I32>,
    ) -> Result<(), BuildError> {
        match self.address_size {
            AddressSize::Bits16 => self.state.write_register(
                &mut self.body,
                Register::named(register),
                value.truncate::<I16>(),
            ),
            AddressSize::Bits32 => self.state.write_register(&mut self.body, register, value),
        }
    }
}
