//! Physical x87 slots preserve their padding while values and tags move.

use std::mem::size_of;

use wasm86_compiler::{BlockBuilder, BuildError, Mem, Val, I1, I16, I32};

use crate::{
    ssa::StateFields,
    state::{access::cpu_location, StoredX87Register},
    x87::ExtendedValue,
};

use super::payload;

#[derive(Clone)]
pub(super) struct Backing {
    memory: Mem,
    tags: StateFields,
}

impl Backing {
    pub(super) fn new(memory: Mem) -> Self {
        Self {
            memory,
            tags: StateFields::new(memory),
        }
    }

    pub(super) fn tag(
        &mut self,
        body: &mut BlockBuilder<'_>,
        slot: &Val<I32>,
    ) -> Result<Val<I16>, BuildError> {
        let tags = self.tags.read(body, cpu_location!(x87.tag_word))?;
        body.value(tags.unsigned().shr(slot.shl(1)).and(3))
    }

    pub(super) fn set_tag(
        &mut self,
        body: &mut BlockBuilder<'_>,
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
        body: &mut BlockBuilder<'_>,
        slot: &Val<I32>,
    ) -> Result<ExtendedValue, BuildError> {
        let address = slot.mul(size_of::<StoredX87Register>() as u32);
        payload::load(body, self.memory, &address)
    }

    pub(super) fn write(
        &mut self,
        body: &mut BlockBuilder<'_>,
        slot: &Val<I32>,
        value: &ExtendedValue,
        tag: Val<I16>,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let address = slot.mul(size_of::<StoredX87Register>() as u32);
        body.if_(enabled, |mut arm| {
            payload::store(&mut arm, self.memory, &address, value)
        })?;
        self.set_tag(body, slot, tag, enabled)
    }

    pub(super) fn initialize(&mut self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        self.tags.define(body, cpu_location!(x87.tag_word), 0xffff)
    }

    pub(super) fn publish(&self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        self.tags.publish(body)
    }
}
