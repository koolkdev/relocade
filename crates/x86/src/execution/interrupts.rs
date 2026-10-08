//! The execution profile gates interrupt state; retirement is shared by all paths.

use super::*;

impl ExecutionBuilder<'_, '_> {
    pub(super) fn retire_instruction(&mut self) {
        if self.profile() == ExecutionProfile::Real16 {
            self.state.interrupts.retire();
        }
        self.completed += 1;
    }

    /// Arms the delay only if this instruction successfully retires.
    pub(crate) fn inhibit_interrupts(&mut self, condition: impl Into<Val<I1>>) {
        if self.profile() == ExecutionProfile::Real16 {
            self.state.interrupts.inhibit(condition);
        }
    }

    /// Architectural event delivery ends inhibition, even if vectoring faults.
    /// A host exit without delivery does not call this operation.
    pub(crate) fn clear_interrupt_shadow(&mut self) {
        if self.profile() == ExecutionProfile::Real16 {
            self.state.interrupts.clear();
        }
    }
}
