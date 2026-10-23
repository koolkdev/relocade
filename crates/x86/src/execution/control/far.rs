//! Far jumps, calls and returns end the current execution entry.
//!
//! Protected CALL and all returns require complete operand-sized slots in SS.
//! Real CALL checks only the transferred span. Backing checks and transfers cover
//! the values, including two selector bytes; dword selector padding stays untouched.
//! This combines RET's full-slot capacity check with P6 selector-transfer behavior;
//! their descriptions leave the access extent ambiguous. See Intel SDM Volume 3B,
//! section 22.31.1:
//! <https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-3b-part-2-manual.pdf#page=575>.

use relocade_compiler::{AtLeast, BuildError, Val, I16, I32};

use crate::{
    exception::Exception,
    execution::{segments::ResolvedSegment, ExecutionBuilder},
    flags::{image, Flag},
    register::RegisterType,
    ExecutionProfile, Segment,
};

use super::Transfer;

/// A resolved CS and offset. Callers must validate its limit before commitment,
/// scheduling that check relative to their other architectural checks.
pub(super) struct CodeTarget {
    offset: Val<I32>,
    segment: ResolvedSegment,
}

impl CodeTarget {
    pub(super) fn resolve<T: RegisterType>(
        execution: &mut ExecutionBuilder<'_, '_>,
        offset: Val<T>,
        selector: &Val<I16>,
    ) -> Result<Self, BuildError>
    where
        I32: AtLeast<T>,
    {
        let offset = offset.unsigned().extend::<I32>();
        let segment = execution.resolve_segment(Segment::Cs, selector)?;
        Ok(Self { offset, segment })
    }

    pub(super) fn check_limit(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
    ) -> Result<(), BuildError> {
        // The new limit applies even when the incoming profile was flat.
        execution.fault_if(
            self.segment.limit().unsigned().lt(&self.offset),
            Exception::GeneralProtection { error_code: 0 },
        )
    }

    /// Installs CS and returns its offset after the caller has checked the limit
    /// and completed all other instruction guards.
    pub(super) fn commit(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
    ) -> Result<Val<I32>, BuildError> {
        self.segment.commit(execution)?;
        Ok(self.offset)
    }
}

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn jump_far<T: RegisterType>(
        &mut self,
        offset: Val<T>,
        selector: Val<I16>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        let target = CodeTarget::resolve(self, offset, &selector)?;
        target.check_limit(self)?;
        let target = target.commit(self)?;
        self.transfer = Transfer::Jump(target);
        Ok(())
    }

    pub(crate) fn call_far<T: RegisterType>(
        &mut self,
        offset: Val<T>,
        selector: Val<I16>,
        return_eip: Val<I32>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        let target = CodeTarget::resolve(self, offset, &selector)?;
        // Real-mode CALL checks the transferred offset and selector. A dword
        // selector slot still reserves four bytes, but its high word is untouched.
        let checked_bytes = match self.profile() {
            ExecutionProfile::Real16 => T::BYTES + 2,
            ExecutionProfile::Protected(_) => 2 * T::BYTES,
        };
        let frame = self.push_frame(2 * T::BYTES, checked_bytes)?;
        target.check_limit(self)?;
        // Prove both fields in push order before writing either of them.
        let selector_slot = frame.field::<I16>(self, T::BYTES)?;
        let offset_slot = frame.field::<T>(self, 0)?;
        let old_cs = self.read_segment_selector(Segment::Cs)?;
        selector_slot.write(self, &old_cs)?;
        offset_slot.write(self, &return_eip.truncate::<T>())?;
        frame.commit(self, 0)?;
        let target = target.commit(self)?;
        self.transfer = Transfer::Call(target);
        Ok(())
    }

    pub(crate) fn return_far<T: RegisterType>(
        &mut self,
        discard_bytes: Val<I16>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        let frame = self.pop_frame(2 * T::BYTES, 2 * T::BYTES)?;
        let offset = frame.field::<T>(self, 0)?.read(self)?;
        let selector = frame.field::<I16>(self, T::BYTES)?.read(self)?;
        let target = resolve_return_target(self, offset, &selector)?;
        frame.commit(self, discard_bytes.unsigned().extend::<I32>())?;
        let target = target.commit(self)?;
        self.transfer = Transfer::Return(target);
        Ok(())
    }

    pub(crate) fn return_interrupt<T: RegisterType>(&mut self) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        match self.profile() {
            ExecutionProfile::Protected(_) => {
                // Only protected mode interprets entry NT as a task return.
                let nested_task = self.read_flag(Flag::NT)?;
                self.unsupported_if(nested_task, 0xcf)?;
            }
            ExecutionProfile::Real16 => {}
        }
        let frame = self.pop_frame(3 * T::BYTES, 3 * T::BYTES)?;
        let offset = frame.field::<T>(self, 0)?.read(self)?;
        match self.profile() {
            ExecutionProfile::Real16 => {
                // IRETD checks EIP before reading CS or FLAGS. Both current and
                // destination Real16 code segments have the canonical 64 KiB limit.
                self.check_code_target(&offset.unsigned().extend::<I32>(), None)?;
            }
            ExecutionProfile::Protected(_) => {}
        }
        let selector = frame.field::<I16>(self, T::BYTES)?.read(self)?;
        let flags = frame.field::<T>(self, 2 * T::BYTES)?.read(self)?;
        let target = match self.profile() {
            ExecutionProfile::Real16 => CodeTarget::resolve(self, offset, &selector)?,
            ExecutionProfile::Protected(_) => resolve_return_target(self, offset, &selector)?,
        };
        image::write_stack_image(self, &flags)?;
        frame.commit(self, 0)?;
        let target = target.commit(self)?;
        self.transfer = Transfer::Return(target);
        Ok(())
    }
}

fn resolve_return_target<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    offset: Val<T>,
    selector: &Val<I16>,
) -> Result<CodeTarget, BuildError>
where
    I32: AtLeast<T>,
{
    if matches!(execution.profile(), ExecutionProfile::Protected(_)) {
        // Protected-mode returns cannot go inward. With CPL fixed at 3, only
        // RPL 3 is valid; real-mode segment values have no privilege bits.
        execution.fault_if(
            selector.and(3).ne(3),
            crate::ExceptionValues::general_protection(
                selector.unsigned().extend::<I32>().and(0xfffc),
            ),
        )?;
    }
    let target = CodeTarget::resolve(execution, offset, selector)?;
    target.check_limit(execution)?;
    Ok(target)
}
