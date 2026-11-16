//! Patterns use the field owner that also receives architectural writes.

use super::{ExecutionContext, State, StatePattern};
use crate::ssa::StateFields;
use relocade_compiler::{BlockBuilder, BuildError, Val, I1};
use std::mem::{offset_of, size_of};

impl State<'_> {
    pub(crate) fn matches(
        &mut self,
        body: &mut BlockBuilder<'_>,
        pattern: &[StatePattern],
    ) -> Result<Val<I1>, BuildError> {
        let mut matches: Val<I1> = true.into();
        for field in pattern {
            matches = matches.and(field.matches(body, self.pattern_fields(*field))?);
        }
        Ok(matches)
    }

    /// Admission or a successful guard must establish the entire pattern.
    pub(crate) fn assume(
        &mut self,
        body: &mut BlockBuilder<'_>,
        pattern: &[StatePattern],
    ) -> Result<(), BuildError> {
        for field in pattern {
            field.apply(body, self.pattern_fields(*field))?;
        }
        Ok(())
    }

    fn pattern_fields(&mut self, pattern: StatePattern) -> &mut StateFields {
        macro_rules! within {
            ($($field:ident).+, $ty:ty) => {
                pattern.within_record(offset_of!(ExecutionContext, cpu.$($field).+), size_of::<$ty>())
            };
        }
        if within!(x87.control, super::X87Control) {
            &mut self.x87.control.fields
        } else if within!(x87.status, super::X87Status) {
            &mut self.x87.status.fields
        } else if within!(simd, super::SimdState) {
            &mut self.simd
        } else {
            // These fields are read-only until a transition dispatches. Other
            // state owners must join this routing before accepting patterns.
            assert!(
                within!(segments, super::Segments)
                    || within!(cr0, u32)
                    || within!(cr4, u32)
                    || within!(flags.bytes.vm, u8)
            );
            &mut self.fields
        }
    }
}
