//! Prospective code targets separate CS resolution, offset checks and commitment.

use wasm86_compiler::{AtLeast, BuildError, Val, I16, I32};

use crate::{
    exception::Exception,
    execution::{segments::ResolvedSegment, ExecutionBuilder},
    register::RegisterType,
    Segment,
};

/// A resolved CS with an offset that still requires a limit check.
/// Instructions schedule that check relative to their other fault checks.
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
            Exception::GeneralProtection {
                error_code: 0.into(),
            },
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
