//! Protected user-mode transfers resolve CS and commit it after all fault checks.
//!
//! Frame compatibility policy: all operand-sized slots must fit SS, but paging
//! and transfers cover only their values, including two selector bytes. Dword
//! selector padding stays untouched. This combines RET's full-slot capacity check
//! with P6 selector-transfer behavior; their descriptions leave the access extent
//! ambiguous. See Intel SDM Volume 3B, section 22.31.1:
//! <https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-3b-part-2-manual.pdf#page=575>.

use wasm86_compiler::{AtLeast, BuildError, Val, I16, I32};

use crate::{
    exception::Exception,
    execution::{segments::ResolvedSegment, ExecutionBuilder},
    flags::{image, Flag},
    register::RegisterType,
    Segment,
};

/// A resolved code segment and offset. Direct transfers check the limit separately
/// because CALL must first validate stack capacity; returns check it during resolution.
struct CodeTarget {
    offset: Val<I32>,
    segment: ResolvedSegment,
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

    fn resolve_return<T: RegisterType>(
        execution: &mut ExecutionBuilder<'_, '_>,
        offset: Val<T>,
        selector: &Val<I16>,
    ) -> Result<Self, BuildError>
    where
        I32: AtLeast<T>,
    {
        // Returns cannot go inward. With CPL fixed at 3, only RPL 3 is valid.
        // After this check the direct-CS resolver implements the return policy.
        execution.fault_if(
            selector.and(3).ne(3),
            Exception::GeneralProtection {
                error_code: selector.unsigned().extend::<I32>().and(0xfffc),
            },
        )?;
        let target = Self::resolve(execution, offset, selector)?;
        target.check_limit(execution)?;
        Ok(target)
    }

    fn check_limit(&self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
        // The new limit applies even when the incoming profile was flat.
        execution.fault_if(
            self.segment.limit().unsigned().lt(&self.offset),
            Exception::GeneralProtection {
                error_code: 0.into(),
            },
        )
    }

    fn commit(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<Val<I32>, BuildError> {
        self.segment.commit(execution)?;
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
        let target = CodeTarget::resolve_return(self, offset, &selector)?;
        frame.commit(self, discard_bytes.unsigned().extend::<I32>())?;
        target.commit(self)
    }

    pub(crate) fn return_interrupt<T: RegisterType>(&mut self) -> Result<Val<I32>, BuildError>
    where
        I32: AtLeast<T>,
    {
        // NT selects a task return before stack access. Task switching uses the
        // unsupported-execution exit.
        let nested_task = self.read_flag(Flag::NT)?;
        self.unsupported_if(nested_task, 0xcf)?;
        let frame = self.pop_frame(3 * T::BYTES, 3 * T::BYTES)?;
        let offset = frame.field::<T>(self, 0)?.read(self)?;
        let selector = frame.field::<I16>(self, T::BYTES)?.read(self)?;
        let flags = frame.field::<T>(self, 2 * T::BYTES)?.read(self)?;
        let target = CodeTarget::resolve_return(self, offset, &selector)?;
        self.write_flags(image::stack_change(&flags))?;
        frame.commit(self, 0)?;
        target.commit(self)
    }
}
