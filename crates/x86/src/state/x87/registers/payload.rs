//! Retain complete x87 values; only physical reads and publication use field layout.

use std::mem::offset_of;

use wasm86_compiler::{BlockBuilder, BuildError, Mem, Val, I1, I16, I32, I64};

use crate::{
    state::{CpuState, StoredX87Register},
    x87::{ExtendedBits, ExtendedValue},
};

/// `value` precedes a pending conditional write. A physical read has no known
/// narrow representation; later definitions retain the entire numerical value.
#[derive(Clone)]
pub(super) struct Payload {
    memory: Mem,
    address: Val<I32>,
    value: Option<ExtendedValue>,
    dirty: bool,
    conditional_write: Option<ConditionalWrite>,
}

#[derive(Clone)]
struct ConditionalWrite {
    enabled: Val<I1>,
    value: ExtendedValue,
}

impl Payload {
    pub(super) fn new(memory: Mem, address: Val<I32>) -> Self {
        Self {
            memory,
            address,
            value: None,
            dirty: false,
            conditional_write: None,
        }
    }

    fn define(&mut self, value: ExtendedValue) {
        self.value = Some(value);
        self.dirty = true;
    }

    pub(super) fn read(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<ExtendedValue, BuildError> {
        if self.value.is_none() {
            self.value = Some(load(body, self.memory, &self.address)?);
        }
        let previous = self.value.as_ref().unwrap();
        Ok(match &self.conditional_write {
            Some(write) => write.value.select(&write.enabled, previous),
            None => previous.clone(),
        })
    }

    /// A false guard must establish a pending x87 exception. Keep it until the
    /// exception check proves this write succeeded, or publication resolves it.
    pub(super) fn write(
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
                self.define(previous);
            }
        }
        self.conditional_write = Some(ConditionalWrite {
            enabled: enabled.clone(),
            value: value.clone(),
        });
        Ok(())
    }

    pub(super) fn discard_write_guard(&mut self) {
        if let Some(write) = self.conditional_write.take() {
            self.define(write.value);
        }
    }

    fn publish_value(&self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        if self.dirty {
            store(
                body,
                self.memory,
                &self.address,
                self.value.as_ref().unwrap(),
            )?;
        }
        Ok(())
    }

    pub(super) fn publish(&self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        match &self.conditional_write {
            Some(write) => body.if_else(
                &write.enabled,
                |mut enabled_body| {
                    store(&mut enabled_body, self.memory, &self.address, &write.value)
                },
                |mut suppressed_body| self.publish_value(&mut suppressed_body),
            ),
            None => self.publish_value(body),
        }
    }
}

pub(super) fn load(
    body: &mut BlockBuilder<'_>,
    memory: Mem,
    address: &Val<I32>,
) -> Result<ExtendedValue, BuildError> {
    let base = offset_of!(CpuState, x87.registers) as u32;
    Ok(ExtendedValue::from_bits(ExtendedBits {
        significand: body.load_at::<I64>(
            memory,
            address,
            base + offset_of!(StoredX87Register, significand) as u32,
        )?,
        sign_exponent: body.load_at::<I16>(
            memory,
            address,
            base + offset_of!(StoredX87Register, sign_exponent) as u32,
        )?,
    }))
}

pub(super) fn store(
    body: &mut BlockBuilder<'_>,
    memory: Mem,
    address: &Val<I32>,
    value: &ExtendedValue,
) -> Result<(), BuildError> {
    let base = offset_of!(CpuState, x87.registers) as u32;
    let bits = value.bits();
    body.store_at::<I64>(
        memory,
        address,
        base + offset_of!(StoredX87Register, significand) as u32,
        bits.significand,
    )?;
    body.store_at::<I16>(
        memory,
        address,
        base + offset_of!(StoredX87Register, sign_exponent) as u32,
        bits.sign_exponent,
    )
}
