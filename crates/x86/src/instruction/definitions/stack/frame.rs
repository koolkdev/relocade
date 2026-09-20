//! Frame instructions keep pointer changes prospective until their accesses succeed.

use wasm86_compiler::{AtLeast, BuildError, I16, I32, I8};

use crate::{
    execution::ExecutionBuilder,
    instruction::{Input, TypedLocation},
    register::{Gpr32, RegisterType},
};

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
    let saved = pointer.clone().push_frame(execution, T::BYTES, T::BYTES)?;
    let frame_pointer = saved.next_pointer().value();
    saved
        .field::<T>(execution, 0)?
        .write(execution, &old_frame.truncate::<T>())?;

    let display_end = execution.if_value::<I32>(
        level.ne(0),
        |nested| {
            let (_, _, esp) = nested.loop_until::<(I32, I32, I32)>(
                (level.sub(1), old_frame, &frame_pointer),
                |(remaining, _, _)| remaining.eq(0),
                |iteration, (remaining, source, esp)| {
                    let source = pointer.with_offset(source).advance(-(T::BYTES as i32));
                    let read = source.clone().pop_frame(iteration, T::BYTES, T::BYTES)?;
                    let value = read.field::<T>(iteration, 0)?.read(iteration)?;
                    let write =
                        pointer
                            .with_offset(esp)
                            .push_frame(iteration, T::BYTES, T::BYTES)?;
                    // Copies observe earlier pushes when the two frames overlap.
                    write.field::<T>(iteration, 0)?.write(iteration, &value)?;
                    Ok((
                        remaining.sub(1),
                        source.offset(),
                        write.next_pointer().value(),
                    ))
                },
            )?;
            let link = pointer
                .with_offset(esp)
                .push_frame(nested, T::BYTES, T::BYTES)?;
            link.field::<T>(nested, 0)?
                .write(nested, &frame_pointer.truncate::<T>())?;
            Ok(link.next_pointer().value())
        },
        |_| Ok(frame_pointer.clone()),
    )?;

    let allocated = pointer.with_offset(display_end.sub(allocation.unsigned().extend::<I32>()));
    // ENTER probes a write at the allocated pointer without storing. Earlier
    // pushes remain visible if this check or a display copy faults.
    allocated.check_write(execution, T::BYTES)?;
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
