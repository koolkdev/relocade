//! Real-mode entry shared by software interrupts and external requests.

use super::{CodeTarget, ExecutionBuilder};
use crate::{
    flags::{image, Flag},
    ExecutionProfile, Segment,
};
use relocade_compiler::{BuildError, Val, I1, I16, I32, I8};

impl ExecutionBuilder<'_, '_> {
    /// Saves a word frame and transfers through the canonical real-mode IVT.
    /// The caller supplies the return IP: software INT uses its fallthrough;
    /// an external interrupt resumes the current, unexecuted instruction.
    pub(crate) fn enter_interrupt(
        &mut self,
        vector: Val<I8>,
        return_ip: Val<I32>,
    ) -> Result<Val<I32>, BuildError> {
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
        ip_slot.write(self, &return_ip.truncate::<I16>())?;

        // Read after the pushes: the stack may alias the vector table.
        let address = vector.unsigned().extend::<I32>().shl(2);
        let selector = self.read_linear_memory::<I16>(address.add(2))?;
        let offset = self.read_linear_memory::<I16>(address)?;
        let target = CodeTarget::resolve(self, offset, &selector)?;
        target.check_limit(self)?;
        frame.commit(self, 0)?;
        target.commit(self)
    }

    /// External entry changes architectural state without retiring an instruction.
    /// The embedding has already accepted and acknowledged the request.
    pub(crate) fn deliver_interrupt(mut self, vector: Val<I8>) -> Result<(), BuildError> {
        let target = self.enter_interrupt(vector, self.eip.clone())?;
        self.state.publish(&mut self.body, target, 0)?;
        self.runtime.dispatch(self.body)
    }

    pub(crate) fn inhibit_interrupts(
        &mut self,
        condition: impl Into<Val<I1>>,
    ) -> Result<(), BuildError> {
        if self.profile() == ExecutionProfile::Real16 {
            self.protect_next = self.body.value(condition.into())?;
        }
        Ok(())
    }
}
