//! Live status fields use the same split backing layout as host snapshots.

use wasm86_compiler::{BlockBuilder, BuildError, Mem, Val, I1, I16, I32, I8};

use crate::{
    ssa::{Location, StateFields},
    state::access::cpu_location,
};

use super::control::{Control, Exception};

impl Exception {
    fn status_location(self) -> Location<I8> {
        match self {
            Self::Invalid => cpu_location!(x87.status.invalid),
            Self::Denormal => cpu_location!(x87.status.denormal),
            Self::ZeroDivide => cpu_location!(x87.status.zero_divide),
            Self::Overflow => cpu_location!(x87.status.overflow),
            Self::Underflow => cpu_location!(x87.status.underflow),
            Self::Precision => cpu_location!(x87.status.precision),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Status {
    fields: StateFields,
}

impl Status {
    pub(super) fn new(memory: Mem) -> Self {
        Self {
            fields: StateFields::new(memory),
        }
    }

    /// Constructs the architectural word only for an instruction that observes it.
    pub(super) fn word(&mut self, body: &mut BlockBuilder<'_>) -> Result<Val<I16>, BuildError> {
        let top = self.top(body)?;
        let mut word = top.truncate::<I16>().shl(11);
        for exception in Exception::ALL {
            let bit = self.exception_raised(body, exception)?;
            word = word.or(bit.unsigned().extend::<I16>().shl(exception as u32));
        }
        for (location, shift) in [
            (cpu_location!(x87.status.stack_fault), 6),
            (cpu_location!(x87.status.error_summary), 7),
            (cpu_location!(x87.status.c0), 8),
            (cpu_location!(x87.status.c1), 9),
            (cpu_location!(x87.status.c2), 10),
            (cpu_location!(x87.status.c3), 14),
            (cpu_location!(x87.status.busy), 15),
        ] {
            let bit = self.flag(body, location)?;
            word = word.or(bit.unsigned().extend::<I16>().shl(shift));
        }
        body.value(word)
    }

    pub(super) fn top(&mut self, body: &mut BlockBuilder<'_>) -> Result<Val<I32>, BuildError> {
        let top = self.fields.read(body, cpu_location!(x87.status.top))?;
        body.value(top.and(7).unsigned().extend::<I32>())
    }

    pub(super) fn set_top(
        &mut self,
        body: &mut BlockBuilder<'_>,
        top: Val<I32>,
        enabled: impl Into<Val<I1>>,
    ) -> Result<(), BuildError> {
        let previous = self.fields.read(body, cpu_location!(x87.status.top))?;
        self.fields.define(
            body,
            cpu_location!(x87.status.top),
            enabled.into().select(top.and(7).truncate::<I8>(), previous),
        )
    }

    pub(crate) fn set_c1(
        &mut self,
        body: &mut BlockBuilder<'_>,
        value: impl Into<Val<I1>>,
    ) -> Result<(), BuildError> {
        self.fields.define(
            body,
            cpu_location!(x87.status.c1),
            value.into().unsigned().extend::<I8>(),
        )
    }

    pub(super) fn pending(&mut self, body: &mut BlockBuilder<'_>) -> Result<Val<I1>, BuildError> {
        self.flag(body, cpu_location!(x87.status.error_summary))
    }

    pub(super) fn exception_raised(
        &mut self,
        body: &mut BlockBuilder<'_>,
        exception: Exception,
    ) -> Result<Val<I1>, BuildError> {
        self.flag(body, exception.status_location())
    }

    /// Recomputes ES and B from existing exception flags and the control word's
    /// masks. Delivery remains deferred until a waiting instruction observes ES.
    pub(super) fn update_pending_exception(
        &mut self,
        body: &mut BlockBuilder<'_>,
        control: &mut Control,
    ) -> Result<(), BuildError> {
        let mut pending = Val::<I1>::from(false);
        for exception in Exception::ALL {
            let raised = self.exception_raised(body, exception)?;
            pending = pending.or(raised.and(control.unmasked(body, exception)?));
        }
        let pending = pending.unsigned().extend::<I8>();
        self.fields
            .define(body, cpu_location!(x87.status.error_summary), &pending)?;
        self.fields
            .define(body, cpu_location!(x87.status.busy), pending)
    }

    /// Records this occurrence and returns whether it is unmasked. Existing
    /// sticky flags do not create a new pending exception here.
    pub(crate) fn record_exception(
        &mut self,
        body: &mut BlockBuilder<'_>,
        exception: Exception,
        raised: &Val<I1>,
        control: &mut Control,
    ) -> Result<Val<I1>, BuildError> {
        self.record_sticky_flag(body, exception.status_location(), raised)?;
        let unmasked = control.unmasked(body, exception)?;
        body.value(raised.and(unmasked))
    }

    pub(crate) fn record_stack_fault(
        &mut self,
        body: &mut BlockBuilder<'_>,
        raised: &Val<I1>,
    ) -> Result<(), BuildError> {
        self.record_sticky_flag(body, cpu_location!(x87.status.stack_fault), raised)
    }

    /// Sets ES and B for a newly unmasked exception; delivery remains deferred.
    pub(crate) fn record_pending_exception(
        &mut self,
        body: &mut BlockBuilder<'_>,
        unmasked: Val<I1>,
    ) -> Result<(), BuildError> {
        for location in [
            cpu_location!(x87.status.error_summary),
            cpu_location!(x87.status.busy),
        ] {
            // A new unmasked fault sets both bits. Other operations preserve
            // their independent imported values, including unused backing bits.
            let previous = self.fields.read(body, location.clone())?;
            self.fields
                .define(body, location, unmasked.select(1_u32, previous))?;
        }
        Ok(())
    }

    fn flag(
        &mut self,
        body: &mut BlockBuilder<'_>,
        location: Location<I8>,
    ) -> Result<Val<I1>, BuildError> {
        Ok(self.fields.read(body, location)?.truncate::<I1>())
    }

    fn record_sticky_flag(
        &mut self,
        body: &mut BlockBuilder<'_>,
        location: Location<I8>,
        raised: &Val<I1>,
    ) -> Result<(), BuildError> {
        let previous = self.fields.read(body, location.clone())?;
        self.fields.define(
            body,
            location,
            previous.or(raised.unsigned().extend::<I8>()),
        )
    }

    pub(super) fn clear_exceptions(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<(), BuildError> {
        for exception in Exception::ALL {
            self.fields.define(body, exception.status_location(), 0)?;
        }
        for location in [
            cpu_location!(x87.status.stack_fault),
            cpu_location!(x87.status.error_summary),
            cpu_location!(x87.status.busy),
        ] {
            self.fields.define(body, location, 0)?;
        }
        Ok(())
    }

    pub(super) fn initialize(&mut self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        self.clear_exceptions(body)?;
        for location in [
            cpu_location!(x87.status.top),
            cpu_location!(x87.status.c0),
            cpu_location!(x87.status.c1),
            cpu_location!(x87.status.c2),
            cpu_location!(x87.status.c3),
        ] {
            self.fields.define(body, location, 0)?;
        }
        Ok(())
    }

    pub(super) fn publish(&self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        self.fields.publish(body)
    }
}
