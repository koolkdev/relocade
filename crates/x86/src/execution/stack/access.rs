//! Stack accesses share a span query while retaining ordered slow transfers.

use relocade_compiler::{BuildError, Val, I1, I32};

use super::{ExecutionBuilder, StackField, StackPointer};
use crate::{
    memory::{Access, Intent},
    register::RegisterType,
    segment::Segment,
};

pub(crate) struct StackAccess {
    pub(crate) access: Access,
    pointer: StackPointer,
    slot_bytes: u32,
}

impl StackAccess {
    /// A direct field is a view into the shared access. Otherwise, check this
    /// slot at its actual use so wrapping, faults and device routing stay ordered.
    pub(crate) fn field<T: RegisterType>(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        index: impl Into<Val<I32>>,
    ) -> Result<StackField<T>, BuildError> {
        assert_eq!(T::BYTES, self.slot_bytes);
        let offset = index.into().mul(T::BYTES);
        let mut access = self.access.field::<T>(&offset)?;
        let (linear, physical, unavailable) = execution.if_value::<(I32, I32, I1)>(
            &self.access.unavailable,
            |slow| {
                let checked = slow.checked(
                    &Segment::Ss.into(),
                    &self.pointer.advance(&offset).offset(),
                    T::BYTES,
                    self.access.intent,
                )?;
                Ok((checked.linear, checked.physical, checked.unavailable))
            },
            |_| Ok((access.linear.clone(), access.physical.clone(), false.into())),
        )?;
        access.linear = linear;
        access.physical = physical;
        access.unavailable = unavailable;
        access.denied = false.into();
        Ok(StackField {
            access,
            marker: std::marker::PhantomData,
        })
    }
}

impl StackPointer {
    /// Probes an entire sequence without transferring data or raising faults.
    /// A slot's bytes stay consecutive, but SP may wrap between successive slots.
    pub(crate) fn check_access<T: RegisterType>(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        count: impl Into<Val<I32>>,
        intent: Intent,
    ) -> Result<StackAccess, BuildError> {
        let count = count.into();
        let bytes = count.mul(T::BYTES);
        let offset = self.offset();
        let last_slot = bytes.sub(T::BYTES);
        let contiguous = offset.add(&last_slot).eq(self.advance(last_slot).offset());
        let segment = execution.state.segments().check(
            &mut execution.body,
            &Segment::Ss.into(),
            &offset,
            &bytes,
            intent,
        )?;
        let empty = count.eq(0);
        let mut access = execution.accesses.memory().check_access(
            &mut execution.body,
            &segment.linear,
            bytes,
            intent,
            Some(
                segment
                    .denied
                    .unwrap_or(false.into())
                    .or(contiguous.eq(false))
                    .or(&empty),
            ),
            None,
        )?;
        access.denied = access.denied.and(empty.eq(false));
        access.unavailable = access.unavailable.and(empty.eq(false));
        Ok(StackAccess {
            access,
            pointer: self.clone(),
            slot_bytes: T::BYTES,
        })
    }
}
