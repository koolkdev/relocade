//! Frame instructions keep pointer changes prospective until their accesses succeed.

use relocade_compiler::{AtLeast, BuildError, Val, I16, I32, I8};

use crate::{
    execution::{ExecutionBuilder, StackAccess, StackPointer},
    instruction::{Input, TypedLocation},
    memory::Intent,
    register::{Gpr32, RegisterType},
    ExecutionProfile,
};

/// The push frame and display keep one availability decision across the copies.
struct EnterAccess {
    pushes: StackAccess,
    display: StackAccess,
}

impl EnterAccess {
    fn prepare<T: RegisterType>(
        execution: &mut ExecutionBuilder<'_, '_>,
        pointer: &StackPointer,
        old_frame: &Val<I32>,
        level: &Val<I32>,
    ) -> Result<Self, BuildError> {
        let push_count = level.eq(0).select(1, level.add(1));
        let display_count = level.eq(0).select(0, level.sub(1));
        let mut pushes = pointer
            .advance(Val::<I32>::from(0).sub(push_count.mul(T::BYTES)))
            .check_access::<T>(execution, push_count, Intent::Write)?;
        let mut display = pointer
            .with_offset(old_frame)
            .advance(Val::<I32>::from(0).sub(display_count.mul(T::BYTES)))
            .check_access::<T>(execution, display_count, Intent::Read)?;
        execution.guard_accesses(true, [&mut pushes.access, &mut display.access])?;
        Ok(Self { pushes, display })
    }
}

pub(super) fn enter_frame<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    allocation: Input<I16>,
    nesting: Input<I8>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let allocation = allocation.read(execution)?;
    let nesting = nesting.read(execution)?;
    let level = nesting.unsigned().extend::<I32>().and(31);
    let old_frame = TypedLocation::<I32>::register(Gpr32::Ebp).read(execution)?;
    let pointer = execution.stack_pointer()?;
    let access = EnterAccess::prepare::<T>(execution, &pointer, &old_frame, &level)?;
    let frame_pointer = pointer.advance(-(T::BYTES as i32)).value();
    access
        .pushes
        .field::<T>(execution, &level)?
        .write(execution, &old_frame.truncate::<T>())?;

    let display_end = execution.if_value::<I32>(
        level.ne(0),
        |nested| {
            let (_, esp) = nested.loop_until::<(I32, I32)>(
                (level.sub(1), &frame_pointer),
                |(remaining, _)| remaining.eq(0),
                |iteration, (remaining, esp)| {
                    // Read after earlier pushes, including through overlapping frames.
                    let value = access
                        .display
                        .field::<T>(iteration, remaining.sub(1))?
                        .read(iteration)?;
                    access
                        .pushes
                        .field::<T>(iteration, &remaining)?
                        .write(iteration, &value)?;
                    Ok((
                        remaining.sub(1),
                        pointer.with_offset(esp).advance(-(T::BYTES as i32)).value(),
                    ))
                },
            )?;
            access
                .pushes
                .field::<T>(nested, 0)?
                .write(nested, &frame_pointer.truncate::<T>())?;
            Ok(pointer.with_offset(esp).advance(-(T::BYTES as i32)).value())
        },
        |_| Ok(frame_pointer.clone()),
    )?;

    let allocated = pointer.with_offset(display_end.sub(allocation.unsigned().extend::<I32>()));
    // Protected-mode ENTER probes the allocation without storing. Real16's
    // 16-bit SP always fits its 64 KiB SS, and unused allocation needs no backing.
    // Earlier pushes remain visible if a check or display copy faults.
    if matches!(execution.profile(), ExecutionProfile::Protected(_)) {
        allocated.check_write(execution, T::BYTES)?;
    }
    TypedLocation::<T>::register(Gpr32::Ebp).write(execution, frame_pointer.truncate::<T>())?;
    allocated.commit(execution)
}

pub(super) fn leave_frame<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
) -> Result<(), BuildError> {
    let frame_pointer = TypedLocation::<I32>::register(Gpr32::Ebp).read(execution)?;
    let pointer = execution.stack_pointer()?.with_offset(frame_pointer);
    // SS.B selects SP/ESP independently of the popped BP/EBP width. Keep
    // the replacement prospective until the frame read has succeeded.
    let frame = pointer.pop_frame(execution, T::BYTES, T::BYTES)?;
    let saved_frame_pointer = frame.field::<T>(execution, 0)?.read(execution)?;
    frame.commit(execution, 0)?;
    TypedLocation::<T>::register(Gpr32::Ebp).write(execution, saved_frame_pointer)
}
