//! Control fields remain independent until a guest observes the control word.

use wasm86_compiler::{BuildError, FunctionBuilder, Mem, Val, I1, I16, I8};

use crate::{
    ssa::{Environment, Location},
    state::access::cpu_location,
};

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
    environment: Environment,
}

impl Control {
    pub(super) fn new(memory: Mem) -> Self {
        Self {
            environment: Environment::new(memory),
        }
    }

    pub(super) fn unmasked(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        exception: Exception,
    ) -> Result<Val<I1>, BuildError> {
        let mask = self.environment.read(body, exception.mask_location())?;
        Ok(mask.truncate::<I1>().eq(false))
    }

    /// Packing preserves reserved bits for readback without letting unused
    /// bits in any backing field affect another architectural control field.
    pub(super) fn word(&mut self, body: &mut FunctionBuilder<'_>) -> Result<Val<I16>, BuildError> {
        let reserved = self
            .environment
            .read(body, cpu_location!(x87.control.reserved_bits))?;
        let mut word = reserved.and(0xe0c0);
        for exception in Exception::ALL {
            let mask = self.environment.read(body, exception.mask_location())?;
            word = word.or(mask.and(1).unsigned().extend::<I16>().shl(exception as u32));
        }
        for (location, mask, shift) in Self::mode_fields() {
            let field = self.environment.read(body, location)?;
            word = word.or(field.and(mask).unsigned().extend::<I16>().shl(shift));
        }
        body.value(word)
    }

    /// FLDCW and FNINIT replace every architectural field, retaining only the
    /// host snapshot's padding. Changing PC does not round existing stack values.
    pub(super) fn load_word(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        word: Val<I16>,
    ) -> Result<(), BuildError> {
        for exception in Exception::ALL {
            self.environment.define(
                body,
                exception.mask_location(),
                word.unsigned()
                    .shr(exception as u32)
                    .and(1)
                    .truncate::<I8>(),
            )?;
        }
        for (location, mask, shift) in Self::mode_fields() {
            self.environment.define(
                body,
                location,
                word.unsigned().shr(shift).and(mask).truncate::<I8>(),
            )?;
        }
        self.environment.define(
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

    pub(super) fn publish(&self, body: &mut FunctionBuilder<'_>) -> Result<(), BuildError> {
        self.environment.publish(body)
    }
}
