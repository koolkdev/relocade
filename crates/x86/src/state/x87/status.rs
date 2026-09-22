//! Live status fields use the same split backing layout as host snapshots.

use wasm86_compiler::{BuildError, FunctionBuilder, Mem, Val, I1, I16, I32, I8};

use crate::{ssa::Environment, state::access::cpu_location};

#[derive(Clone)]
pub(super) struct Status {
    environment: Environment,
}

impl Status {
    pub(super) fn new(memory: Mem) -> Self {
        Self {
            environment: Environment::new(memory),
        }
    }

    /// Constructs the architectural word only for an instruction that observes it.
    pub(super) fn word(&mut self, body: &mut FunctionBuilder<'_>) -> Result<Val<I16>, BuildError> {
        let exceptions = self
            .environment
            .read(body, cpu_location!(x87.status.exception_flags))?;
        let top = self.top(body)?;
        let mut word = exceptions
            .and(0x7f)
            .unsigned()
            .extend::<I16>()
            .or(top.truncate::<I16>().shl(11));
        for (location, shift) in [
            (cpu_location!(x87.status.error_summary), 7),
            (cpu_location!(x87.status.c0), 8),
            (cpu_location!(x87.status.c1), 9),
            (cpu_location!(x87.status.c2), 10),
            (cpu_location!(x87.status.c3), 14),
            (cpu_location!(x87.status.busy), 15),
        ] {
            let bit = self.environment.read(body, location)?;
            word = word.or(bit.and(1).unsigned().extend::<I16>().shl(shift));
        }
        body.value(word)
    }

    pub(super) fn top(&mut self, body: &mut FunctionBuilder<'_>) -> Result<Val<I32>, BuildError> {
        let top = self.environment.read(body, cpu_location!(x87.status.top))?;
        body.value(top.and(7).unsigned().extend::<I32>())
    }

    pub(super) fn set_top(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        top: Val<I32>,
        enabled: impl Into<Val<I1>>,
    ) -> Result<(), BuildError> {
        let previous = self.environment.read(body, cpu_location!(x87.status.top))?;
        self.environment.define(
            body,
            cpu_location!(x87.status.top),
            enabled.into().select(top.and(7).truncate::<I8>(), previous),
        )
    }

    pub(super) fn set_c1(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        value: impl Into<Val<I1>>,
    ) -> Result<(), BuildError> {
        self.environment.define(
            body,
            cpu_location!(x87.status.c1),
            value.into().unsigned().extend::<I8>(),
        )
    }

    pub(super) fn pending(
        &mut self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<Val<I1>, BuildError> {
        let summary = self
            .environment
            .read(body, cpu_location!(x87.status.error_summary))?;
        Ok(summary.and(1).ne(0))
    }

    /// Recomputes ES and B from existing exception flags and the control word's
    /// masks. Delivery remains deferred until a waiting instruction observes ES.
    pub(super) fn update_pending_exception(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        control: &Val<I16>,
    ) -> Result<(), BuildError> {
        let exceptions = self
            .environment
            .read(body, cpu_location!(x87.status.exception_flags))?;
        let pending = exceptions
            .and(0x3f)
            .unsigned()
            .extend::<I16>()
            .and(control.xor(0xffff))
            .ne(0)
            .unsigned()
            .extend::<I8>();
        self.environment
            .define(body, cpu_location!(x87.status.error_summary), &pending)?;
        self.environment
            .define(body, cpu_location!(x87.status.busy), pending)
    }

    pub(super) fn stack_fault(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        fault: &Val<I1>,
        unmasked: &Val<I1>,
        overflow: Val<I1>,
    ) -> Result<(), BuildError> {
        let exceptions = self
            .environment
            .read(body, cpu_location!(x87.status.exception_flags))?;
        self.environment.define(
            body,
            cpu_location!(x87.status.exception_flags),
            exceptions.or(fault.select(0x41_u32, 0_u32)),
        )?;
        for location in [
            cpu_location!(x87.status.error_summary),
            cpu_location!(x87.status.busy),
        ] {
            // A new unmasked fault sets both bits. Other operations preserve
            // their independent imported values, including unused backing bits.
            let previous = self.environment.read(body, location.clone())?;
            self.environment
                .define(body, location, unmasked.select(1_u32, previous))?;
        }
        self.set_c1(body, overflow)
    }

    pub(super) fn clear_exceptions(
        &mut self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<(), BuildError> {
        for location in [
            cpu_location!(x87.status.exception_flags),
            cpu_location!(x87.status.error_summary),
            cpu_location!(x87.status.busy),
        ] {
            self.environment.define(body, location, 0)?;
        }
        Ok(())
    }

    pub(super) fn initialize(&mut self, body: &mut FunctionBuilder<'_>) -> Result<(), BuildError> {
        for location in [
            cpu_location!(x87.status.exception_flags),
            cpu_location!(x87.status.top),
            cpu_location!(x87.status.c0),
            cpu_location!(x87.status.c1),
            cpu_location!(x87.status.c2),
            cpu_location!(x87.status.c3),
            cpu_location!(x87.status.error_summary),
            cpu_location!(x87.status.busy),
        ] {
            self.environment.define(body, location, 0)?;
        }
        Ok(())
    }

    pub(super) fn publish(&self, body: &mut FunctionBuilder<'_>) -> Result<(), BuildError> {
        self.environment.publish(body)
    }
}
