//! Concatenated GPR values retain each register's independent architectural view.

use wasm86_compiler::{BuildError, Val};

use super::ExecutionBuilder;
use crate::{
    alu::DoubleWidth,
    register::{Gpr32, Register, RegisterType},
};

impl ExecutionBuilder<'_, '_> {
    /// Reads high:low as one unsigned bit pattern, extending each half separately.
    pub(crate) fn read_register_pair<T: RegisterType + DoubleWidth>(
        &mut self,
        high: Gpr32,
        low: Gpr32,
    ) -> Result<Val<T::Double>, BuildError> {
        let low = self
            .state
            .read_register(&mut self.body, Register::<T>::named(low))?;
        let high = self
            .state
            .read_register(&mut self.body, Register::<T>::named(high))?;
        Ok(low
            .unsigned()
            .extend::<T::Double>()
            .or(high.unsigned().extend::<T::Double>().shl(T::BYTES * 8)))
    }

    /// Writes low then high through the existing register-alias mechanism.
    pub(crate) fn write_register_pair<T: RegisterType + DoubleWidth>(
        &mut self,
        high: Gpr32,
        low: Gpr32,
        value: Val<T::Double>,
    ) -> Result<(), BuildError> {
        self.state.write_register(
            &mut self.body,
            Register::<T>::named(low),
            value.truncate::<T>(),
        )?;
        self.state.write_register(
            &mut self.body,
            Register::<T>::named(high),
            value.unsigned().shr(T::BYTES * 8).truncate::<T>(),
        )
    }
}
