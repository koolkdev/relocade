//! Prospective code targets separate CS resolution, offset checks and commitment.

use relocade_compiler::{AtLeast, BuildError, Val, I16, I32};

use crate::{
    exception::Exception,
    execution::{segments::ResolvedSegment, ExecutionBuilder},
    register::RegisterType,
    ExecutionProfile, Segment,
};

/// A resolved CS and offset. Callers must validate its limit before commitment,
/// scheduling that check relative to their other architectural checks.
pub(crate) struct CodeTarget {
    offset: Val<I32>,
    segment: ResolvedSegment,
}

impl CodeTarget {
    pub(crate) fn resolve<T: RegisterType>(
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

    pub(crate) fn check_limit(
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
    pub(crate) fn commit(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
    ) -> Result<Val<I32>, BuildError> {
        self.segment.commit(execution)?;
        Ok(self.offset)
    }
}

impl ExecutionBuilder<'_, '_> {
    /// Reserves optional host argument space above the ordinary return address.
    pub(crate) fn call_far<T: RegisterType>(
        &mut self,
        offset: Val<T>,
        selector: Val<I16>,
        return_ip: Val<I32>,
        stack_bytes: impl Into<Val<I32>>,
    ) -> Result<Val<I32>, BuildError>
    where
        I32: AtLeast<T>,
    {
        let stack_bytes = stack_bytes.into();
        let target = CodeTarget::resolve(self, offset, &selector)?;
        // Real-mode CALL transfers a word selector even in a dword-sized slot.
        let checked_bytes = match self.profile() {
            ExecutionProfile::Real16 => T::BYTES + 2,
            ExecutionProfile::Protected(_) => 2 * T::BYTES,
        };
        let frame = self.push_frame(
            stack_bytes.add(2 * T::BYTES),
            stack_bytes.add(checked_bytes),
        )?;
        target.check_limit(self)?;
        // Prove both return fields in push order before writing either of them.
        let selector = frame.field::<I16>(self, T::BYTES)?;
        let offset = frame.field::<T>(self, 0)?;
        let old_cs = self.read_segment_selector(Segment::Cs)?;
        selector.write(self, &old_cs)?;
        offset.write(self, &return_ip.truncate::<T>())?;
        frame.commit(self, 0)?;
        let offset = target.commit(self)?;
        // Real16 keeps the same execution profile after loading CS. Protected
        // CS transitions retain ordinary dispatch across the context change.
        if self.profile() == ExecutionProfile::Real16 {
            self.call_dispatch = true;
        }
        Ok(offset)
    }
}
