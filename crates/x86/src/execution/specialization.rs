//! Guard a specialization before refining values on the continuing path.

use super::ExecutionBuilder;
use crate::state::StatePattern;
use relocade_compiler::BuildError;

impl ExecutionBuilder<'_, '_> {
    /// Check the preferred bits at their consumer before using them as known bits.
    /// Paths without interpreter fallback keep their live state.
    pub(crate) fn guard_specialization(
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
