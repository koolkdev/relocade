//! Code fetch and control transfers check CS before instruction effects.

mod far;
mod interrupts;
mod transfer;

pub(super) use transfer::Transfer;

use relocade_compiler::{AtLeast, BuildError, Val, I1, I16, I32};

use crate::{exception::Exception, memory::Intent, register::RegisterType, segment::Segment};

use super::ExecutionBuilder;

impl ExecutionBuilder<'_, '_> {
    /// Checks a snapshot block's complete encoding before any guest effects.
    /// The interpreter handles a denied span to preserve prefix execution and
    /// fault order, including faults earlier than the first invalid CS offset.
    pub(crate) fn guard_fetch_span(&mut self, bytes: usize) -> Result<(), BuildError> {
        let Ok(bytes) = u32::try_from(bytes) else {
            return self.guard(false);
        };
        let check = self.segments.check(
            &mut self.body,
            &Segment::Cs.into(),
            &self.eip,
            bytes,
            Intent::Fetch,
        )?;
        if let Some(denied) = check.denied {
            self.guard(denied.eq(false))?;
        }
        Ok(())
    }

    pub(crate) fn jump(&mut self, target: Val<I32>) -> Result<(), BuildError> {
        self.check_code_target(&target, None)?;
        self.transfer = Transfer::Jump(target);
        Ok(())
    }

    pub(crate) fn branch(
        &mut self,
        taken: Val<I1>,
        target: Val<I32>,
        fallthrough: Val<I32>,
    ) -> Result<(), BuildError> {
        self.check_code_target(&target, Some(&taken))?;
        self.transfer = Transfer::Branch {
            taken,
            target,
            fallthrough,
        };
        Ok(())
    }

    /// The target is captured before the return-address push. Neither a denied
    /// target nor a failed push completes the call.
    pub(crate) fn call<T: RegisterType>(
        &mut self,
        target: Val<I32>,
        return_eip: Val<I32>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        self.check_code_target(&target, None)?;
        self.push(return_eip.truncate::<T>(), T::BYTES)?;
        self.transfer = Transfer::Call(target);
        Ok(())
    }

    pub(crate) fn return_near<T: RegisterType>(
        &mut self,
        discard_bytes: Val<I16>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        let frame = self.pop_frame(T::BYTES, T::BYTES)?;
        let value = frame.field::<T>(self, 0)?.read(self)?;
        let target = value.unsigned().extend::<I32>();
        self.check_code_target(&target, None)?;
        frame.commit(self, discard_bytes.unsigned().extend::<I32>())?;
        self.transfer = Transfer::Return(target);
        Ok(())
    }

    fn check_code_target(
        &mut self,
        target: &Val<I32>,
        taken: Option<&Val<I1>>,
    ) -> Result<(), BuildError> {
        let check = self.segments.check(
            &mut self.body,
            &Segment::Cs.into(),
            target,
            1,
            Intent::Fetch,
        )?;
        if let Some(denied) = check.denied {
            let fault = match taken {
                Some(taken) => taken.and(denied),
                None => denied,
            };
            self.fault_if(fault, Exception::GeneralProtection { error_code: 0 })?;
        }
        Ok(())
    }
}
