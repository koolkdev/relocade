//! Control fields remain independent until a guest observes the control word.

use wasm86_compiler::{BlockBuilder, BuildError, Mem, Val, I1, I16, I8};

use crate::{
    ssa::{Location, StateFields},
    state::{access::cpu_location, StoredX87Control},
    x87::RoundingMode,
};

pub(crate) enum X87ModeFields {
    Rounding,
    PrecisionAndRounding,
}

/// Maskable exceptions share bit positions in the architectural control and
/// status words. Stack fault is a status condition, not a seventh exception mask.
#[derive(Clone, Copy)]
pub(super) enum Exception {
    Invalid = 0,
    Denormal = 1,
    ZeroDivide = 2,
    Overflow = 3,
    Underflow = 4,
    Precision = 5,
}

impl Exception {
    pub(super) const ALL: [Self; 6] = [
        Self::Invalid,
        Self::Denormal,
        Self::ZeroDivide,
        Self::Overflow,
        Self::Underflow,
        Self::Precision,
    ];

    fn mask_location(self) -> Location<I8> {
        match self {
            Self::Invalid => cpu_location!(x87.control.invalid_mask),
            Self::Denormal => cpu_location!(x87.control.denormal_mask),
            Self::ZeroDivide => cpu_location!(x87.control.zero_divide_mask),
            Self::Overflow => cpu_location!(x87.control.overflow_mask),
            Self::Underflow => cpu_location!(x87.control.underflow_mask),
            Self::Precision => cpu_location!(x87.control.precision_mask),
        }
    }
}

#[derive(Clone)]
pub(super) struct Control {
    fields: StateFields,
}

impl Control {
    pub(super) fn new(memory: Mem) -> Self {
        Self {
            fields: StateFields::new(memory),
        }
    }

    pub(super) fn rounding(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<RoundingMode, BuildError> {
        Ok(RoundingMode::new(
            self.fields
                .read(body, cpu_location!(x87.control.rounding_control))?,
        ))
    }

    pub(super) fn precision(&mut self, body: &mut BlockBuilder<'_>) -> Result<Val<I8>, BuildError> {
        self.fields
            .read(body, cpu_location!(x87.control.precision_control))
    }

    pub(super) fn matches_mode(
        &mut self,
        body: &mut BlockBuilder<'_>,
        expected: &StoredX87Control,
        fields: X87ModeFields,
    ) -> Result<Val<I1>, BuildError> {
        let rounding = self
            .fields
            .read(body, cpu_location!(x87.control.rounding_control))?;
        let matches = rounding.and(3).eq(u32::from(expected.rounding_control & 3));
        Ok(match fields {
            X87ModeFields::Rounding => matches,
            X87ModeFields::PrecisionAndRounding => matches.and(
                self.precision(body)?
                    .and(3)
                    .eq(u32::from(expected.precision_control & 3)),
            ),
        })
    }

    pub(super) fn unmasked(
        &mut self,
        body: &mut BlockBuilder<'_>,
        exception: Exception,
    ) -> Result<Val<I1>, BuildError> {
        let mask = self.fields.read(body, exception.mask_location())?;
        Ok(mask.truncate::<I1>().eq(false))
    }

    /// Packing preserves reserved bits for readback without letting unused
    /// bits in any backing field affect another architectural control field.
    pub(super) fn word(&mut self, body: &mut BlockBuilder<'_>) -> Result<Val<I16>, BuildError> {
        let reserved = self
            .fields
            .read(body, cpu_location!(x87.control.reserved_bits))?;
        let mut word = reserved.and(0xe0c0);
        for exception in Exception::ALL {
            let mask = self.fields.read(body, exception.mask_location())?;
            word = word.or(mask.and(1).unsigned().extend::<I16>().shl(exception as u32));
        }
        for (location, mask, shift) in Self::mode_fields() {
            let field = self.fields.read(body, location)?;
            word = word.or(field.and(mask).unsigned().extend::<I16>().shl(shift));
        }
        body.value(word)
    }

    /// FLDCW and FNINIT replace every architectural field, retaining only the
    /// host snapshot's padding. Changing PC does not round existing stack values.
    pub(super) fn load_word(
        &mut self,
        body: &mut BlockBuilder<'_>,
        word: Val<I16>,
    ) -> Result<(), BuildError> {
        for exception in Exception::ALL {
            self.fields.define(
                body,
                exception.mask_location(),
                word.unsigned()
                    .shr(exception as u32)
                    .and(1)
                    .truncate::<I8>(),
            )?;
        }
        for (location, mask, shift) in Self::mode_fields() {
            self.fields.define(
                body,
                location,
                word.unsigned().shr(shift).and(mask).truncate::<I8>(),
            )?;
        }
        self.fields.define(
            body,
            cpu_location!(x87.control.reserved_bits),
            word.and(0xe0c0),
        )
    }

    fn mode_fields() -> [(Location<I8>, u32, u32); 3] {
        [
            (cpu_location!(x87.control.precision_control), 3, 8),
            (cpu_location!(x87.control.rounding_control), 3, 10),
            (cpu_location!(x87.control.infinity_control), 1, 12),
        ]
    }

    pub(super) fn publish(&self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        self.fields.publish(body)
    }
}
