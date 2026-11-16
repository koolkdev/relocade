//! Preferred CPU state is guarded before it becomes an assumption.

use super::ExecutionBuilder;
use crate::state::StatePattern;
use relocade_compiler::BuildError;

impl ExecutionBuilder<'_, '_> {
    /// Paths without interpreter fallback keep their live state.
    pub(crate) fn specialize_state(
        &mut self,
        preferred: &[StatePattern],
    ) -> Result<(), BuildError> {
        self.if_can_fallback(|jit| {
            let matches = jit.state.matches(&mut jit.body, preferred)?;
            jit.guard(matches)?;
            jit.state.assume(&mut jit.body, preferred)
        })?;
        Ok(())
    }
}
