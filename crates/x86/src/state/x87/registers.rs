//! Physical x87 slots preserve their padding while values and tags move.

use std::mem::{offset_of, size_of};

use wasm86_compiler::{BuildError, FunctionBuilder, Mem, Val, I1, I16, I32, I64};

use crate::{
    ssa::Environment,
    state::{access::cpu_location, CpuState, StoredX87Register},
};

use super::ExtendedValue;

#[derive(Clone)]
pub(super) struct Registers {
    memory: Mem,
    tags: Environment,
}

impl Registers {
    pub(super) fn new(memory: Mem) -> Self {
        Self {
            memory,
            tags: Environment::new(memory),
        }
    }

    pub(super) fn tag(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        slot: &Val<I32>,
    ) -> Result<Val<I16>, BuildError> {
        let tags = self.tags.read(body, cpu_location!(x87.tag_word))?;
        body.value(tags.unsigned().shr(slot.shl(1)).and(3))
    }

    pub(super) fn set_tag(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        slot: &Val<I32>,
        tag: Val<I16>,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let tags = self.tags.read(body, cpu_location!(x87.tag_word))?;
        let shift = slot.shl(1);
        let mask = Val::<I16>::from(3_u32).shl(&shift);
        let updated = tags.and(mask.xor(0xffff)).or(tag.shl(shift));
        self.tags.define(
            body,
            cpu_location!(x87.tag_word),
            enabled.select(updated, tags),
        )
    }

    pub(super) fn read(
        &self,
        body: &mut FunctionBuilder<'_>,
        slot: &Val<I32>,
    ) -> Result<ExtendedValue, BuildError> {
        let address = slot.mul(size_of::<StoredX87Register>() as u32);
        let base = offset_of!(CpuState, x87.registers) as u32;
        Ok(ExtendedValue {
            significand: body.load_at::<I64>(
                self.memory,
                &address,
                base + offset_of!(StoredX87Register, significand) as u32,
            )?,
            sign_exponent: body.load_at::<I16>(
                self.memory,
                address,
                base + offset_of!(StoredX87Register, sign_exponent) as u32,
            )?,
        })
    }

    pub(super) fn write(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        slot: &Val<I32>,
        value: &ExtendedValue,
        tag: Val<I16>,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let address = slot.mul(size_of::<StoredX87Register>() as u32);
        let base = offset_of!(CpuState, x87.registers) as u32;
        body.if_(enabled, |mut arm| {
            arm.store_at::<I64>(
                self.memory,
                &address,
                base + offset_of!(StoredX87Register, significand) as u32,
                &value.significand,
            )?;
            arm.store_at::<I16>(
                self.memory,
                &address,
                base + offset_of!(StoredX87Register, sign_exponent) as u32,
                &value.sign_exponent,
            )
        })?;
        self.set_tag(body, slot, tag, enabled)
    }

    pub(super) fn initialize(&mut self, body: &mut FunctionBuilder<'_>) -> Result<(), BuildError> {
        self.tags.define(body, cpu_location!(x87.tag_word), 0xffff)
    }

    pub(super) fn publish(&self, body: &mut FunctionBuilder<'_>) -> Result<(), BuildError> {
        self.tags.publish(body)
    }
}
