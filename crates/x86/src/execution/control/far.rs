//! Protected user-mode transfers resolve CS and commit it after all fault checks.
//!
//! Frame compatibility policy: both operand-sized slots must fit SS, but paging
//! and transfers cover only the offset and two selector bytes. Dword selector
//! padding stays untouched. This combines RET's full-slot capacity check with
//! P6 selector-transfer behavior; their descriptions leave the access extent
//! ambiguous. See Intel SDM Volume 3B, section 22.31.1:
//! <https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-3b-part-2-manual.pdf#page=575>.

use wasm86_compiler::{AtLeast, BuildError, Val, I16, I32};

use crate::{
    exception::Exception,
    execution::ExecutionBuilder,
    register::RegisterType,
    segment::{Segment, SegmentValues},
};

/// Resolution proves code type, privilege and presence; the offset is checked later
/// because CALL must first validate its stack capacity.
struct CodeTarget {
    offset: Val<I32>,
    segment: SegmentValues,
}

impl CodeTarget {
    fn resolve<T: RegisterType>(
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

    fn check_limit(&self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
        // The new limit applies even when the incoming profile was flat.
        execution.fault_if(
            self.segment.limit.unsigned().lt(&self.offset),
            Exception::GeneralProtection {
                error_code: 0.into(),
            },
        )
    }

    fn commit(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<Val<I32>, BuildError> {
        execution
            .state
            .write_segment(&mut execution.body, &Segment::Cs.into(), &self.segment)?;
        Ok(self.offset)
    }
}

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn jump_far<T: RegisterType>(
        &mut self,
        offset: Val<T>,
        selector: Val<I16>,
    ) -> Result<Val<I32>, BuildError>
    where
        I32: AtLeast<T>,
    {
        let target = CodeTarget::resolve(self, offset, &selector)?;
        target.check_limit(self)?;
        target.commit(self)
    }

    pub(crate) fn call_far<T: RegisterType>(
        &mut self,
        offset: Val<T>,
        selector: Val<I16>,
        fallthrough: Val<I32>,
    ) -> Result<Val<I32>, BuildError>
    where
        I32: AtLeast<T>,
    {
        let target = CodeTarget::resolve(self, offset, &selector)?;
        let frame = self.push_frame(2 * T::BYTES, 2 * T::BYTES)?;
        target.check_limit(self)?;
        // Both slots must fit SS. The selector's unused high word is not touched.
        // Prove both fields in push order before writing either of them.
        let selector_slot = frame.field::<I16>(self, T::BYTES)?;
        let offset_slot = frame.field::<T>(self, 0)?;
        let old_cs = self.read_segment_selector(Segment::Cs)?;
        selector_slot.write(self, &old_cs)?;
        offset_slot.write(self, &fallthrough.truncate::<T>())?;
        frame.commit(self, 0)?;
        target.commit(self)
    }

    pub(crate) fn return_far<T: RegisterType>(
        &mut self,
        discard_bytes: Val<I16>,
    ) -> Result<Val<I32>, BuildError>
    where
        I32: AtLeast<T>,
    {
        let frame = self.pop_frame(2 * T::BYTES, 2 * T::BYTES)?;
        let offset = frame.field::<T>(self, 0)?.read(self)?;
        let selector = frame.field::<I16>(self, T::BYTES)?.read(self)?;
        // RET cannot return inward. With CPL fixed at 3, only RPL 3 is valid.
        // After this check the direct-CS resolver also implements RET's policy.
        self.fault_if(
            selector.and(3).ne(3),
            Exception::GeneralProtection {
                error_code: selector.unsigned().extend::<I32>().and(0xfffc),
            },
        )?;
        let target = CodeTarget::resolve(self, offset, &selector)?;
        target.check_limit(self)?;
        frame.commit(self, discard_bytes.unsigned().extend::<I32>())?;
        target.commit(self)
    }
}
