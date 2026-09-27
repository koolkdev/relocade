//! Retains logical stack slots while preserving their physical backing and padding.
mod backing;

use std::mem::{offset_of, size_of};

use wasm86_compiler::{BlockBuilder, BuildError, Mem, Val, I1, I16, I32, I64};

use crate::{
    ssa::{Location, StateFields, TrackedValue},
    state::{CpuState, StoredX87Register},
};

use super::ExtendedValue;
use backing::Backing;

#[derive(Clone)]
pub(super) struct Slot {
    pub(super) physical: Val<I32>,
    relative: Option<usize>,
}

#[derive(Clone)]
pub(super) struct Registers {
    memory: Mem,
    backing: Backing,
    frame: Option<Frame>,
    dynamic: bool,
}

/// Slots keep fixed physical addresses relative to the captured TOP. `delta`
/// tracks attempted movement while building the block. A suppressed movement
/// sets the pending-exception flag, so the next stack instruction exits before
/// using that mapping. FNCLEX/FNINIT rebase before subsequent stack accesses.
#[derive(Clone)]
struct Frame {
    entry_top: Val<I32>,
    delta: usize,
    entry_tag_word: Val<I16>,
    tags: [Option<Tag>; 8],
    payloads: [Option<Payload>; 8],
}

impl Frame {
    fn physical(&self, relative: usize) -> Val<I32> {
        self.entry_top.add(relative as u32).and(7)
    }
}

#[derive(Clone)]
struct Tag {
    current: TrackedValue<I16>,
    // Keep the last assignment, including an empty tag after FSTP ST0.
    // The exception check proves its guard before it replaces `current`.
    latest_write: Option<Val<I16>>,
}

/// `fields` holds the value before `conditional_write`, or the current value
/// when there is no conditional write. Reads and publication honor its guard.
#[derive(Clone)]
struct Payload {
    fields: StateFields,
    conditional_write: Option<ConditionalWrite>,
}

#[derive(Clone)]
struct ConditionalWrite {
    enabled: Val<I1>,
    value: ExtendedValue,
}

impl Registers {
    pub(super) fn new(memory: Mem) -> Self {
        Self {
            memory,
            backing: Backing::new(memory),
            frame: None,
            dynamic: false,
        }
    }

    pub(super) fn slot(
        &mut self,
        body: &mut BlockBuilder<'_>,
        top: &Val<I32>,
        index: Val<I32>,
    ) -> Result<Slot, BuildError> {
        let known = (0..8_u32).find(|i| index.same_expression(&Val::from(*i)));
        if known.is_none() && !self.dynamic {
            self.rebase(body)?;
            self.dynamic = true;
        }
        let physical = body.value(top.add(&index).and(7))?;
        if self.dynamic {
            return Ok(Slot {
                physical,
                relative: None,
            });
        }
        if self.frame.is_none() {
            self.backing.publish(body)?;
            self.backing = Backing::new(self.memory);
            self.frame = Some(Frame {
                entry_top: top.clone(),
                delta: 0,
                entry_tag_word: body
                    .load(self.memory, offset_of!(CpuState, x87.tag_word) as u32)?,
                tags: std::array::from_fn(|_| None),
                payloads: std::array::from_fn(|_| None),
            });
        }
        let frame = self.frame.as_ref().unwrap();
        Ok(Slot {
            physical,
            relative: Some((frame.delta + known.unwrap() as usize) & 7),
        })
    }

    pub(super) fn advance(&mut self, delta: i32) {
        if let Some(frame) = &mut self.frame {
            frame.delta = ((frame.delta as i32 + delta) & 7) as usize;
        }
    }

    pub(super) fn tag(
        &mut self,
        body: &mut BlockBuilder<'_>,
        slot: &Slot,
    ) -> Result<Val<I16>, BuildError> {
        let Some(relative) = slot.relative else {
            return self.backing.tag(body, &slot.physical);
        };
        let frame = self.frame.as_mut().unwrap();
        if let Some(tag) = &frame.tags[relative] {
            return tag.current.read(body);
        }
        let initial = frame
            .entry_tag_word
            .unsigned()
            .shr(frame.physical(relative).shl(1))
            .and(3);
        let tag = Tag {
            current: TrackedValue::new(body, initial)?,
            latest_write: None,
        };
        let value = tag.current.read(body)?;
        frame.tags[relative] = Some(tag);
        Ok(value)
    }

    pub(super) fn set_tag(
        &mut self,
        body: &mut BlockBuilder<'_>,
        slot: &Slot,
        tag: Val<I16>,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let Some(relative) = slot.relative else {
            return self.backing.set_tag(body, &slot.physical, tag, enabled);
        };
        let previous = self.tag(body, slot)?;
        let cached_tag = self.frame.as_mut().unwrap().tags[relative]
            .as_mut()
            .unwrap();
        cached_tag
            .current
            .define(body, enabled.select(&tag, previous))?;
        cached_tag.latest_write = Some(body.value(tag)?);
        Ok(())
    }

    fn payload(
        &mut self,
        body: &mut BlockBuilder<'_>,
        relative: usize,
    ) -> Result<&mut Payload, BuildError> {
        let frame = self.frame.as_mut().unwrap();
        if frame.payloads[relative].is_none() {
            let address = body.value(
                frame
                    .physical(relative)
                    .mul(size_of::<StoredX87Register>() as u32),
            )?;
            frame.payloads[relative] = Some(Payload::new(self.memory, address));
        }
        Ok(frame.payloads[relative].as_mut().unwrap())
    }

    pub(super) fn read(
        &mut self,
        body: &mut BlockBuilder<'_>,
        slot: &Slot,
    ) -> Result<ExtendedValue, BuildError> {
        match slot.relative {
            Some(relative) => self.payload(body, relative)?.read(body),
            None => self.backing.read(body, &slot.physical),
        }
    }

    pub(super) fn write(
        &mut self,
        body: &mut BlockBuilder<'_>,
        slot: &Slot,
        value: &ExtendedValue,
        tag: Val<I16>,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let Some(relative) = slot.relative else {
            return self
                .backing
                .write(body, &slot.physical, value, tag, enabled);
        };
        self.payload(body, relative)?.write(body, value, enabled)?;
        self.set_tag(body, slot, tag, enabled)
    }

    /// Called by x87's exception check after constructing the terminating fault
    /// branch. A disabled write implies ES = 1; the continuing path has ES = 0,
    /// so its latest writes can be retained without their guards.
    pub(super) fn discard_write_guards(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<(), BuildError> {
        if let Some(frame) = &mut self.frame {
            for tag in frame.tags.iter_mut().flatten() {
                if let Some(value) = tag.latest_write.take() {
                    tag.current.define(body, value)?;
                }
            }
            for payload in frame.payloads.iter_mut().flatten() {
                if let Some(write) = payload.conditional_write.take() {
                    Payload::define_fields(&mut payload.fields, body, &write.value)?;
                }
            }
        }
        Ok(())
    }

    /// Publishes the current values, including conditional writes, and forgets
    /// the slot mapping. FNCLEX/FNINIT and dynamic indexing need fresh tracking;
    /// none of them proves that an earlier conditional write was enabled.
    pub(super) fn rebase(&mut self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        self.publish(body)?;
        self.frame = None;
        self.backing = Backing::new(self.memory);
        self.dynamic = false;
        Ok(())
    }

    pub(super) fn initialize(&mut self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        self.rebase(body)?;
        self.backing.initialize(body)
    }

    pub(super) fn publish(&self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        let Some(frame) = &self.frame else {
            return self.backing.publish(body);
        };
        for payload in frame.payloads.iter().flatten() {
            payload.publish(body)?;
        }
        let mut tags = frame.entry_tag_word.clone();
        let mut changed = false;
        for relative in 0..8 {
            if let Some(tag) = frame.tags[relative]
                .as_ref()
                .and_then(|tag| tag.current.dirty_value())
            {
                changed = true;
                let shift = frame.physical(relative).shl(1);
                tags = tags
                    .and(Val::<I16>::from(3_u32).shl(&shift).xor(0xffff))
                    .or(tag.shl(shift));
            }
        }
        if changed {
            body.store(self.memory, offset_of!(CpuState, x87.tag_word) as u32, tags)?;
        }
        Ok(())
    }
}

impl Payload {
    fn new(memory: Mem, address: Val<I32>) -> Self {
        Self {
            fields: StateFields::with_base(memory, address),
            conditional_write: None,
        }
    }

    fn define_fields(
        fields: &mut StateFields,
        body: &mut BlockBuilder<'_>,
        value: &ExtendedValue,
    ) -> Result<(), BuildError> {
        let base = offset_of!(CpuState, x87.registers) as u32;
        fields.define(
            body,
            Location::<I64>::new(base + offset_of!(StoredX87Register, significand) as u32),
            &value.significand,
        )?;
        fields.define(
            body,
            Location::<I16>::new(base + offset_of!(StoredX87Register, sign_exponent) as u32),
            &value.sign_exponent,
        )
    }

    fn read(&mut self, body: &mut BlockBuilder<'_>) -> Result<ExtendedValue, BuildError> {
        let base = offset_of!(CpuState, x87.registers) as u32;
        let previous = ExtendedValue {
            significand: self.fields.read(
                body,
                Location::<I64>::new(base + offset_of!(StoredX87Register, significand) as u32),
            )?,
            sign_exponent: self.fields.read(
                body,
                Location::<I16>::new(base + offset_of!(StoredX87Register, sign_exponent) as u32),
            )?,
        };
        Ok(match &self.conditional_write {
            Some(write) => ExtendedValue {
                significand: write
                    .enabled
                    .select(&write.value.significand, previous.significand),
                sign_exponent: write
                    .enabled
                    .select(&write.value.sign_exponent, previous.sign_exponent),
            },
            None => previous,
        })
    }

    /// A false `enabled` guard must establish a pending x87 exception (ES = 1).
    /// Keep that guard until the pending-exception check proves the write was
    /// enabled, or publication chooses its value for an exit or tracking reset.
    fn write(
        &mut self,
        body: &mut BlockBuilder<'_>,
        value: &ExtendedValue,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        if let Some(write) = &self.conditional_write {
            if !write.enabled.same_expression(enabled) {
                // Equal guards let the later write replace the earlier one
                // (for example, FXCH ST0). Different guards must retain the
                // earlier conditional result as the new write's fallback.
                let previous = self.read(body)?;
                Self::define_fields(&mut self.fields, body, &previous)?;
            }
        }
        self.conditional_write = Some(ConditionalWrite {
            enabled: enabled.clone(),
            value: value.clone(),
        });
        Ok(())
    }

    fn publish(&self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        match &self.conditional_write {
            Some(write) => body.if_else(
                &write.enabled,
                |mut enabled_body| {
                    let mut fields = self.fields.clone();
                    Self::define_fields(&mut fields, &mut enabled_body, &write.value)?;
                    fields.publish(&mut enabled_body)
                },
                |mut suppressed_body| self.fields.publish(&mut suppressed_body),
            ),
            None => self.fields.publish(body),
        }
    }
}
