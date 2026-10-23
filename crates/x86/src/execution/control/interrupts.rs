//! Real-mode software interrupt entry after the instruction has admitted the mode.

use relocade_compiler::{BuildError, Val, I16, I32, I8};

use super::{far::CodeTarget, Transfer};
use crate::{
    execution::ExecutionBuilder,
    flags::{image, Flag},
    Segment,
};

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn interrupt(
        &mut self,
        vector: Val<I8>,
        return_eip: Val<I32>,
    ) -> Result<(), BuildError> {
        // Real-mode INT always saves three words, including with a 66 prefix.
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
        let target = target.commit(self)?;
        self.transfer = Transfer::Interrupt(target);
        Ok(())
    }
}
