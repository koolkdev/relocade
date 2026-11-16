//! A preferred state pattern becomes usable only on the guarded continuation.

use super::ExecutionBuilder;
use crate::state::StatePattern;
use relocade_compiler::BuildError;

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn specialize_state(&mut self, pattern: &[StatePattern]) -> Result<(), BuildError> {
        self.if_can_fallback(|jit| {
            let matches = jit.state.matches(&mut jit.body, pattern)?;
            jit.guard(matches)?;
            jit.state.assume(&mut jit.body, pattern)
        })?;
        Ok(())
    }
}
