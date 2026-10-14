//! Whole-bank images keep logical payload order separate from physical tag order.

use super::*;
use crate::x87::ExtendedBits;

impl Registers {
    pub(in crate::state::x87) fn tag_word(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I16>, BuildError> {
        self.bank
            .get_or_insert_with(|| Bank::new(self.memory, 0.into()))
            .tag_word(body)
    }

    pub(in crate::state::x87) fn saved_tag_word(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I16>, BuildError> {
        self.bank
            .get_or_insert_with(|| Bank::new(self.memory, 0.into()))
            .saved_tag_word(body)
    }

    pub(in crate::state::x87) fn stack_bits(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<[ExtendedBits; 8], BuildError> {
        // A no-wait save can follow a suppressed stack movement. Resolve logical
        // positions against actual TOP, retaining values when that mapping agrees.
        if let Some(bank) = &mut self.bank {
            bank.stack_top = None;
        }
        let mut values = std::array::from_fn(|_| ExtendedBits {
            significand: 0.into(),
            sign_exponent: 0.into(),
        });
        for (index, value) in values.iter_mut().enumerate() {
            let slot = self.slot(body, (index as u32).into())?;
            *value = self.read(body, &slot)?.bits();
        }
        Ok(values)
    }

    /// A full restore replaces every payload in logical stack order; FLDENV
    /// keeps them in place. Both rebuild occupied tags from the resulting values.
    pub(in crate::state::x87) fn restore(
        &mut self,
        body: &mut BlockBuilder<'_>,
        top: Val<I32>,
        tags: &Val<I16>,
        values: Option<&[ExtendedBits; 8]>,
    ) -> Result<(), BuildError> {
        self.top.define(
            body,
            cpu_location!(x87.status.top),
            top.and(7).truncate::<I8>(),
        )?;
        let origin = if values.is_some() {
            // All old payloads and tags are overwritten. Keeping them would force
            // unnecessary publication and reads when the restored TOP differs.
            let mut bank = Bank::new(self.memory, top.clone());
            bank.stack_top = Some(0);
            bank.reset_tags(body, 0xffff)?;
            self.bank = Some(bank);
            top
        } else {
            if let Some(bank) = &mut self.bank {
                bank.stack_top = None;
            }
            0.into()
        };
        for index in 0..8 {
            let slot = self.locate(
                body,
                Address {
                    origin: origin.clone(),
                    offset: index,
                },
            )?;
            let value = if let Some(values) = values {
                let value = ExtendedValue::from_bits(values[index].clone());
                self.write_value(body, &slot, &value, &true.into())?;
                value
            } else {
                self.read(body, &slot)?
            };
            let physical = origin.add(index as u32).and(7);
            let empty = tags.unsigned().shr(physical.shl(1)).and(3).eq(3);
            let tag = empty.select(3, value.classification().tag());
            self.set_tag(body, &slot, tag, &true.into())?;
        }
        Ok(())
    }
}
