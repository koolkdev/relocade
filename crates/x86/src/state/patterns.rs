//! Patterns use the field owner that also receives architectural writes.

use super::{ExecutionContext, State, StatePattern};
use crate::ssa::StateFields;
use relocade_compiler::{BlockBuilder, BuildError, Val, I1};
use std::mem::{offset_of, size_of};

impl State<'_> {
    pub(crate) fn matches(
        &mut self,
        body: &mut BlockBuilder<'_>,
        patterns: &[StatePattern],
    ) -> Result<Val<I1>, BuildError> {
        let mut matches: Val<I1> = true.into();
        for &pattern in patterns {
            let fields = self.fields_for(pattern);
            matches = matches.and(pattern.matches(body, fields)?);
        }
        Ok(matches)
    }

    /// Admission or a successful guard must establish every supplied fact.
    pub(crate) fn assume(
        &mut self,
        body: &mut BlockBuilder<'_>,
        patterns: &[StatePattern],
    ) -> Result<(), BuildError> {
        for &pattern in patterns {
            let fields = self.fields_for(pattern);
            pattern.apply(body, fields)?;
        }
        Ok(())
    }

    pub(super) fn fields_for(&mut self, pattern: StatePattern) -> &mut StateFields {
        if let Some(fields) = self.x87.fields_for(pattern) {
            return fields;
        }
        macro_rules! within {
            ($($field:ident).+, $ty:ty) => {
                pattern.within_record(offset_of!(ExecutionContext, cpu.$($field).+), size_of::<$ty>())
            };
        }
        if within!(simd, super::SimdState) {
            return &mut self.simd;
        }
        if within!(flags.bytes.if_, u8) || within!(flags.bytes.iopl, u8) {
            return &mut self.flags.direct;
        }
        // These fields are read-only until a transition dispatches. Other
        // state owners must join this routing before accepting patterns.
        assert!(
            within!(segments, super::Segments)
                || within!(cr0, u32)
                || within!(cr4, u32)
                || within!(flags.bytes.vm, u8),
            "pattern field has no retained state owner"
        );
        &mut self.fields
    }
}
