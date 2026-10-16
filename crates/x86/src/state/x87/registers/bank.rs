//! Retained values and tags for eight disjoint slots relative to one origin.

use std::mem::{offset_of, size_of};
use wasm86_compiler::{BlockBuilder, BuildError, Mem, Val, I1, I16, I32};

use super::payload::Payload;
use crate::{
    ssa::TrackedValue,
    state::{ExecutionContext, StoredX87Register},
    x87::ExtendedValue,
};

#[derive(Clone, Default)]
struct Register {
    payload: Option<Payload>,
    tag: Option<Tag>,
}

#[derive(Clone)]
struct Tag {
    current: TrackedValue<I16>,
    // The exception check proves that the latest x87 write was enabled.
    latest_write: Option<Val<I16>>,
}

#[derive(Clone)]
pub(super) struct Bank {
    memory: Mem,
    pub(super) origin: Val<I32>,
    pub(super) stack_top: Option<usize>,
    tag_word: Option<TrackedValue<I16>>,
    registers: [Register; 8],
}

impl Bank {
    pub(super) fn new(memory: Mem, origin: Val<I32>) -> Self {
        Self {
            memory,
            origin,
            stack_top: None,
            tag_word: None,
            registers: std::array::from_fn(|_| Register::default()),
        }
    }

    pub(super) fn physical(&self, relative: usize) -> Val<I32> {
        self.origin.add(relative as u32).and(7)
    }

    fn payload(
        &mut self,
        body: &mut BlockBuilder<'_>,
        relative: usize,
    ) -> Result<&mut Payload, BuildError> {
        if self.registers[relative].payload.is_none() {
            let address = body.value(
                self.physical(relative)
                    .mul(size_of::<StoredX87Register>() as u32),
            )?;
            self.registers[relative].payload = Some(Payload::new(self.memory, address));
        }
        Ok(self.registers[relative].payload.as_mut().unwrap())
    }

    pub(super) fn read(
        &mut self,
        body: &mut BlockBuilder<'_>,
        relative: usize,
    ) -> Result<ExtendedValue, BuildError> {
        self.payload(body, relative)?.read(body)
    }

    pub(super) fn write(
        &mut self,
        body: &mut BlockBuilder<'_>,
        relative: usize,
        value: &ExtendedValue,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        self.payload(body, relative)?.write(body, value, enabled)
    }

    pub(super) fn tag(
        &mut self,
        body: &mut BlockBuilder<'_>,
        relative: usize,
    ) -> Result<Val<I16>, BuildError> {
        if let Some(tag) = &self.registers[relative].tag {
            return tag.current.read(body);
        }
        self.read_tag_word(body)?;
        let initial = self
            .tag_word
            .as_ref()
            .unwrap()
            .value()
            .unsigned()
            .shr(self.physical(relative).shl(1))
            .and(3);
        let current = TrackedValue::new(body, initial)?;
        let value = current.read(body)?;
        self.registers[relative].tag = Some(Tag {
            current,
            latest_write: None,
        });
        Ok(value)
    }

    fn read_tag_word(&mut self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        if self.tag_word.is_none() {
            let word = body.load(
                self.memory,
                offset_of!(ExecutionContext, cpu.x87.tag_word) as u32,
            )?;
            self.tag_word = Some(TrackedValue::new(body, word)?);
        }
        Ok(())
    }

    pub(super) fn tag_word(&mut self, body: &mut BlockBuilder<'_>) -> Result<Val<I16>, BuildError> {
        self.read_tag_word(body)?;
        body.value(self.packed_tags().unwrap().0)
    }

    /// Legacy saves classify occupied payloads, including registers written by
    /// MMX, while preserving the live tags and any unresolved write guards.
    pub(super) fn saved_tag_word(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I16>, BuildError> {
        let mut word = Val::<I16>::from(0);
        for relative in 0..8 {
            let empty = self.tag(body, relative)?.eq(3);
            let value = self.read(body, relative)?;
            let tag = empty.select(3, value.classification().tag());
            word = word.or(tag.shl(self.physical(relative).shl(1)));
        }
        body.value(word)
    }

    pub(super) fn set_tag(
        &mut self,
        body: &mut BlockBuilder<'_>,
        relative: usize,
        tag: Val<I16>,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let previous = self.tag(body, relative)?;
        let cached = self.registers[relative].tag.as_mut().unwrap();
        cached
            .current
            .define(body, enabled.select(&tag, previous))?;
        cached.latest_write = Some(body.value(tag)?);
        Ok(())
    }

    pub(super) fn reset_tags(
        &mut self,
        body: &mut BlockBuilder<'_>,
        tags: u16,
    ) -> Result<(), BuildError> {
        self.tag_word = Some(TrackedValue::defined(body, u32::from(tags))?);
        for register in &mut self.registers {
            register.tag = None;
        }
        Ok(())
    }

    pub(super) fn discard_write_guards(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<(), BuildError> {
        for register in &mut self.registers {
            if let Some(tag) = &mut register.tag {
                if let Some(value) = tag.latest_write.take() {
                    tag.current.define(body, value)?;
                }
            }
            if let Some(payload) = &mut register.payload {
                payload.discard_write_guard();
            }
        }
        Ok(())
    }

    pub(super) fn publish(&self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        for register in &self.registers {
            if let Some(payload) = &register.payload {
                payload.publish(body)?;
            }
        }
        if let Some((tags, true)) = self.packed_tags() {
            body.store(
                self.memory,
                offset_of!(ExecutionContext, cpu.x87.tag_word) as u32,
                tags,
            )?;
        }
        Ok(())
    }

    fn packed_tags(&self) -> Option<(Val<I16>, bool)> {
        let word = self.tag_word.as_ref()?;
        let mut tags = word.value().clone();
        let mut changed = word.dirty_value().is_some();
        for (relative, register) in self.registers.iter().enumerate() {
            if let Some(tag) = register
                .tag
                .as_ref()
                .and_then(|tag| tag.current.dirty_value())
            {
                changed = true;
                let shift = self.physical(relative).shl(1);
                tags = tags
                    .and(Val::<I16>::from(3_u32).shl(&shift).xor(0xffff))
                    .or(tag.shl(shift));
            }
        }
        Some((tags, changed))
    }
}
