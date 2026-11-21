//! Code fetch and control transfers check CS before instruction effects.

mod far;

pub(crate) use far::CodeTarget;

use relocade_compiler::{AtLeast, BuildError, Val, I1, I32};

use crate::{
    exception::Exception,
    memory::Intent,
    register::RegisterType,
    segment::{Segment, SegmentDefaultSize},
};

use super::{transfer::Transfer, ExecutionBuilder};

impl ExecutionBuilder<'_, '_> {
    /// Checks the loaded CS.D used for decoding and the complete fetch span before guest effects.
    /// The interpreter handles a denied span to preserve prefix execution and
    /// fault order, including faults earlier than the first invalid CS offset.
    pub(crate) fn guard_snapshot(
        &mut self,
        code_default_size: SegmentDefaultSize,
        bytes: usize,
    ) -> Result<(), BuildError> {
        let is_32_bit = self.code_default_is_32_bit()?;
        self.guard(is_32_bit.eq(code_default_size == SegmentDefaultSize::Bits32))?;
        let Ok(bytes) = u32::try_from(bytes) else {
            return self.guard(false);
        };
        let check = self.state.segments().check(
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

    pub(crate) fn jump(&mut self, target: Val<I32>) -> Result<Val<I32>, BuildError> {
        self.check_code_target(&target, None)?;
        self.transfer = Transfer::Jump;
        Ok(target)
    }

    pub(crate) fn return_(&mut self, target: Val<I32>) -> Result<Val<I32>, BuildError> {
        self.check_code_target(&target, None)?;
        self.transfer = Transfer::Return;
        Ok(target)
    }

    /// Completes a near call after checking its target and return-address push.
    pub(crate) fn call<T: RegisterType>(
        &mut self,
        target: Val<I32>,
        return_address: Val<I32>,
    ) -> Result<Val<I32>, BuildError>
    where
        I32: AtLeast<T>,
    {
        self.check_code_target(&target, None)?;
        self.push(return_address.truncate::<T>(), T::BYTES)?;
        self.transfer = Transfer::Call {
            link: self.call_link()?,
            can_enter_callee: true.into(),
        };
        Ok(target)
    }

    pub(crate) fn branch(
        &mut self,
        taken: Val<I1>,
        target: Val<I32>,
        fallthrough: Val<I32>,
    ) -> Result<Val<I32>, BuildError> {
        self.check_code_target(&target, Some(&taken))?;
        self.transfer = Transfer::Branch {
            condition: taken.clone(),
            taken: target.clone(),
            fallthrough: fallthrough.clone(),
        };
        Ok(taken.select(target, fallthrough))
    }

    fn check_code_target(
        &mut self,
        target: &Val<I32>,
        taken: Option<&Val<I1>>,
    ) -> Result<(), BuildError> {
        let check = self.state.segments().check(
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
