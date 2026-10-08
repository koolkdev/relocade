//! The execution profile gates interrupt state; retirement is shared by all paths.

use super::*;
use crate::{flags::image, Segment};

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn halt(&mut self) {
        debug_assert_eq!(self.profile(), ExecutionProfile::Real16);
        self.state.halted = Some(true);
    }

    /// An accepted event wakes the CPU before any delivery fault can occur.
    pub(crate) fn wake(&mut self) {
        debug_assert_eq!(self.profile(), ExecutionProfile::Real16);
        self.state.halted = Some(false);
    }

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

    /// Enters the canonical real-mode vector table from an instruction or host event.
    /// The caller supplies the saved IP and owns instruction retirement, if any.
    pub(crate) fn enter_real_mode_interrupt(
        &mut self,
        vector: Val<I8>,
        return_eip: Val<I32>,
    ) -> Result<Val<I32>, BuildError> {
        debug_assert_eq!(self.profile(), ExecutionProfile::Real16);
        self.clear_interrupt_shadow();
        let frame = self.push_frame(6, 6)?;
        let flags_slot = frame.field::<I16>(self, 4)?;
        let cs_slot = frame.field::<I16>(self, 2)?;
        let ip_slot = frame.field::<I16>(self, 0)?;
        let flags = image::read_stack_image::<I16>(self)?;
        let cs = self.read_segment_selector(Segment::Cs)?;
        flags_slot.write(self, &flags)?;
        self.write_flag(Flag::IF, false)?;
        self.write_flag(Flag::TF, false)?;
        self.write_flag(Flag::AC, false)?;
        cs_slot.write(self, &cs)?;
        ip_slot.write(self, &return_eip.truncate::<I16>())?;

        // Real16 uses the conventional 256-entry IVT at linear address zero.
        // Read after the pushes: the stack may alias the vector table.
        let address = vector.unsigned().extend::<I32>().shl(2);
        let selector = self.read_linear_memory::<I16>(address.add(2))?;
        let offset = self.read_linear_memory::<I16>(address)?;
        let target = CodeTarget::resolve(self, offset, &selector)?;
        target.check_limit(self)?;
        frame.commit(self, 0)?;
        target.commit(self)
    }
}
