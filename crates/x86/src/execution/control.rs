//! Control transfers validate their destination before publishing instruction effects.

use wasm86_compiler::{AtLeast, BuildError, Val, I1, I16, I32};

use crate::{exception::Exception, memory::Intent, register::RegisterType, segment::Segment};

use super::ExecutionBuilder;

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn jump_far<T: RegisterType>(
        &mut self,
        offset: Val<T>,
        selector: Val<I16>,
    ) -> Result<Val<I32>, BuildError>
    where
        I32: AtLeast<T>,
    {
        let target = offset.unsigned().extend::<I32>();
        let values = self.resolve_segment(Segment::Cs, &selector)?;
        // Resolution proves that this is a present, accessible code segment.
        // Its limit applies even when the incoming profile was flat.
        self.fault_if(
            values.limit.unsigned().lt(&target),
            Exception::GeneralProtection {
                error_code: 0.into(),
            },
        )?;
        self.state
            .write_segment(&mut self.body, &Segment::Cs.into(), &values)?;
        Ok(target)
    }

    pub(crate) fn jump(&mut self, target: Val<I32>) -> Result<Val<I32>, BuildError> {
        self.check_code_target(&target, None)?;
        Ok(target)
    }

    pub(crate) fn branch(
        &mut self,
        taken: Val<I1>,
        target: Val<I32>,
        fallthrough: Val<I32>,
    ) -> Result<Val<I32>, BuildError> {
        self.check_code_target(&target, Some(&taken))?;
        Ok(taken.select(target, fallthrough))
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
            self.fault_if(
                fault,
                Exception::GeneralProtection {
                    error_code: 0.into(),
                },
            )?;
        }
        Ok(())
    }
}
