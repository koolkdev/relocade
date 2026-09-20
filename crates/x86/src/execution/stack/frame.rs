//! Procedure frames keep pointer changes provisional until every access succeeds.

use wasm86_compiler::{AtLeast, BuildError, Val, I16, I32, I8};

use crate::{
    memory::Intent,
    register::{Gpr32, Register, RegisterType},
    segment::Segment,
};

use super::ExecutionBuilder;

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn enter_frame<T: RegisterType>(
        &mut self,
        allocation: Val<I16>,
        nesting: Val<I8>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        let level = nesting.unsigned().extend::<I32>().and(31);
        let old_frame = self.state.read_register(&mut self.body, Gpr32::Ebp)?;
        let pointer = self.stack_pointer()?;
        let saved = pointer.clone().push_frame(self, T::BYTES, T::BYTES)?;
        let frame_pointer = saved.next_pointer().value();
        saved
            .field::<T>(self, 0)?
            .write(self, &old_frame.truncate::<T>())?;

        let display_end = self.if_value::<I32>(
            level.ne(0),
            |mut nested| {
                let esp = nested.loop_::<(I32, I32, I32), I32>(
                    (level.sub(1), old_frame, &frame_pointer),
                    |mut iteration, labels, (remaining, source, esp)| {
                        iteration
                            .body
                            .branch_if(remaining.eq(0), &labels.exit, &esp)?;
                        let source = pointer.with_offset(source).advance(-(T::BYTES as i32));
                        let read = source
                            .clone()
                            .pop_frame(&mut iteration, T::BYTES, T::BYTES)?;
                        let value = read.field::<T>(&mut iteration, 0)?.read(&mut iteration)?;
                        let write = pointer.with_offset(esp).push_frame(
                            &mut iteration,
                            T::BYTES,
                            T::BYTES,
                        )?;
                        // Copies observe earlier pushes when the two frames overlap.
                        write
                            .field::<T>(&mut iteration, 0)?
                            .write(&mut iteration, &value)?;
                        iteration.body.branch(
                            &labels.again,
                            (
                                remaining.sub(1),
                                source.offset(),
                                write.next_pointer().value(),
                            ),
                        )
                    },
                )?;
                let link = pointer
                    .with_offset(esp)
                    .push_frame(&mut nested, T::BYTES, T::BYTES)?;
                link.field::<T>(&mut nested, 0)?
                    .write(&mut nested, &frame_pointer.truncate::<T>())?;
                nested.body.yield_(link.next_pointer().value())
            },
            |nested| nested.body.yield_(&frame_pointer),
        )?;

        let allocated = pointer.with_offset(display_end.sub(allocation.unsigned().extend::<I32>()));
        let memory = self.memory.expect("ENTER declares guest memory");
        // ENTER probes a write at the allocated stack pointer without storing.
        // Earlier pushes remain visible if this or a display copy faults.
        self.checked(
            memory,
            &Segment::Ss.into(),
            &allocated.offset(),
            T::BYTES,
            Intent::Write,
        )?;
        self.state.write_register(
            &mut self.body,
            Register::<T>::named(Gpr32::Ebp),
            frame_pointer.truncate::<T>(),
        )?;
        allocated.commit(self)
    }
}
