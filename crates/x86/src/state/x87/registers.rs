//! One register bank for stack-relative x87 and physical MMX accesses.
mod bank;
mod payload;

#[cfg(test)]
mod tests;

use wasm86_compiler::{BlockBuilder, BuildError, Mem, Val, I1, I16, I32, I8};

use crate::{
    register::RegisterCode, ssa::StateFields, state::access::cpu_location, x87::ExtendedValue,
};

use bank::Bank;

/// A slot in the current bank; consume it before selecting another address.
#[derive(Clone)]
pub(super) struct Slot {
    relative: usize,
}

/// An address relative to a captured register-bank origin. Known offsets name
/// disjoint slots even when the physical origin is only known at runtime.
struct Address {
    origin: Val<I32>,
    offset: usize,
}

#[derive(Clone)]
pub(super) struct Registers {
    memory: Mem,
    top: StateFields,
    bank: Option<Bank>,
}

fn known_slot(body: &mut BlockBuilder<'_>, value: &Val<I32>) -> Result<Option<usize>, BuildError> {
    Ok(body
        .constant_bits(value)?
        .filter(|&slot| slot < 8)
        .map(|slot| slot as usize))
}

impl Registers {
    pub(super) fn new(memory: Mem) -> Self {
        Self {
            memory,
            top: StateFields::new(memory),
            bank: None,
        }
    }

    pub(super) fn top(&mut self, body: &mut BlockBuilder<'_>) -> Result<Val<I32>, BuildError> {
        let top = self.top.read(body, cpu_location!(x87.status.top))?;
        body.value(top.and(7).unsigned().extend::<I32>())
    }

    pub(super) fn tag_word(&mut self, body: &mut BlockBuilder<'_>) -> Result<Val<I16>, BuildError> {
        self.bank
            .get_or_insert_with(|| Bank::new(self.memory, 0.into()))
            .tag_word(body)
    }

    /// Restoring TOP changes logical positions, not physical payloads. Only empty
    /// tags survive a load verbatim; every occupied register classifies its value.
    pub(super) fn load_environment(
        &mut self,
        body: &mut BlockBuilder<'_>,
        top: Val<I32>,
        tags: &Val<I16>,
    ) -> Result<(), BuildError> {
        self.top.define(
            body,
            cpu_location!(x87.status.top),
            top.and(7).truncate::<I8>(),
        )?;
        if let Some(bank) = &mut self.bank {
            bank.stack_top = None;
        }
        for index in 0..8 {
            let slot = self.physical(body, RegisterCode::from_code(index))?;
            let value = self.read(body, &slot)?;
            let empty = tags.unsigned().shr(u32::from(index) * 2).and(3).eq(3);
            let tag = empty.select(3, value.classification().tag());
            self.set_tag(body, &slot, tag, &true.into())?;
        }
        Ok(())
    }

    pub(super) fn slot(
        &mut self,
        body: &mut BlockBuilder<'_>,
        index: Val<I32>,
    ) -> Result<Slot, BuildError> {
        let top = self.top(body)?;
        let known = known_slot(body, &index)?;
        if let Some(relative) =
            known.and_then(|index| self.bank.as_ref()?.stack_top.map(|top| (top + index) & 7))
        {
            return Ok(Slot { relative });
        }
        let address = match known {
            Some(index) => Address {
                origin: top,
                offset: index,
            },
            None => Address {
                origin: body.value(top.add(index).and(7))?,
                offset: 0,
            },
        };
        let slot = self.locate(body, address)?;
        if let Some(index) = known {
            self.bank.as_mut().unwrap().stack_top = Some((slot.relative + 8 - index) & 7);
        }
        Ok(slot)
    }

    pub(super) fn physical(
        &mut self,
        body: &mut BlockBuilder<'_>,
        register: RegisterCode,
    ) -> Result<Slot, BuildError> {
        let address = match register {
            RegisterCode::Known(code) => Address {
                origin: 0.into(),
                offset: usize::from(code & 7),
            },
            RegisterCode::Indexed(code) => Address {
                origin: body.value(code.and(7))?,
                offset: 0,
            },
        };
        self.locate(body, address)
    }

    fn locate(
        &mut self,
        body: &mut BlockBuilder<'_>,
        address: Address,
    ) -> Result<Slot, BuildError> {
        if self
            .bank
            .as_ref()
            .is_some_and(|bank| bank.origin.same_expression(&address.origin))
        {
            return Ok(Slot {
                relative: address.offset,
            });
        }
        let physical = body.value(address.origin.add(address.offset as u32).and(7))?;
        if let Some(bank) = &self.bank {
            let offset = body.value(physical.sub(&bank.origin).and(7))?;
            if let Some(relative) = known_slot(body, &offset)? {
                return Ok(Slot { relative });
            }
        }
        // A different symbolic origin may alias any retained register. Resolve
        // its pending writes before selecting a new coordinate system.
        self.rebase(body)?;
        let (origin, relative) = match known_slot(body, &physical)? {
            Some(index) => (Val::from(0), index),
            None => (address.origin, address.offset),
        };
        self.bank = Some(Bank::new(self.memory, origin));
        Ok(Slot { relative })
    }

    /// Advance the speculative stack mapping together with architectural TOP.
    /// A suppressed movement sets ES, so the next waiting instruction exits
    /// before using that mapping. FNCLEX/FNINIT rebase without assuming success.
    pub(super) fn advance(
        &mut self,
        body: &mut BlockBuilder<'_>,
        delta: i32,
        enabled: impl Into<Val<I1>>,
    ) -> Result<(), BuildError> {
        let bank_top = self.bank.as_mut().and_then(|bank| {
            let relative = ((bank.stack_top? as i32 + delta) & 7) as usize;
            bank.stack_top = Some(relative);
            Some(bank.physical(relative))
        });
        let updated = match bank_top {
            Some(top) => top,
            None => self.top(body)?.add((delta & 7) as u32).and(7),
        };
        let previous = self.top.read(body, cpu_location!(x87.status.top))?;
        self.top.define(
            body,
            cpu_location!(x87.status.top),
            enabled.into().select(updated.truncate::<I8>(), previous),
        )
    }

    pub(super) fn tag(
        &mut self,
        body: &mut BlockBuilder<'_>,
        slot: &Slot,
    ) -> Result<Val<I16>, BuildError> {
        self.bank.as_mut().unwrap().tag(body, slot.relative)
    }

    pub(super) fn set_tag(
        &mut self,
        body: &mut BlockBuilder<'_>,
        slot: &Slot,
        tag: Val<I16>,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        self.bank
            .as_mut()
            .unwrap()
            .set_tag(body, slot.relative, tag, enabled)
    }

    pub(super) fn read(
        &mut self,
        body: &mut BlockBuilder<'_>,
        slot: &Slot,
    ) -> Result<ExtendedValue, BuildError> {
        self.bank.as_mut().unwrap().read(body, slot.relative)
    }

    pub(super) fn write(
        &mut self,
        body: &mut BlockBuilder<'_>,
        slot: &Slot,
        value: &ExtendedValue,
        tag: Val<I16>,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        self.write_value(body, slot, value, enabled)?;
        self.set_tag(body, slot, tag, enabled)
    }

    pub(super) fn write_value(
        &mut self,
        body: &mut BlockBuilder<'_>,
        slot: &Slot,
        value: &ExtendedValue,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        self.bank
            .as_mut()
            .unwrap()
            .write(body, slot.relative, value, enabled)
    }

    pub(super) fn discard_write_guards(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<(), BuildError> {
        if let Some(bank) = &mut self.bank {
            bank.discard_write_guards(body)?;
        }
        Ok(())
    }

    /// Publish unresolved guards and forget only the register address mapping.
    pub(super) fn rebase(&mut self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        if let Some(bank) = &self.bank {
            bank.publish(body)?;
        }
        self.bank = None;
        Ok(())
    }

    pub(super) fn initialize(&mut self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        self.rebase(body)?;
        self.reset(body, 0xffff)
    }

    /// MMX and EMMS reset TOP and the tag word while retaining every payload.
    pub(super) fn reset(
        &mut self,
        body: &mut BlockBuilder<'_>,
        tags: u16,
    ) -> Result<(), BuildError> {
        self.top
            .define::<I8>(body, cpu_location!(x87.status.top), 0)?;
        let bank = self
            .bank
            .get_or_insert_with(|| Bank::new(self.memory, 0.into()));
        let offset = body.value(Val::<I32>::from(0).sub(&bank.origin).and(7))?;
        bank.stack_top = known_slot(body, &offset)?;
        bank.reset_tags(body, tags)
    }

    pub(super) fn publish(&self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        self.top.publish(body)?;
        if let Some(bank) = &self.bank {
            bank.publish(body)?;
        }
        Ok(())
    }
}
