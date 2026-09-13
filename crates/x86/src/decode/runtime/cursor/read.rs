use wasm86_compiler::{AtLeast, BuildError, FunctionBuilder, MemoryInt, Val, I32, I8};

use crate::{exception::Exception, instruction::MAX_INSTRUCTION_BYTES, state::exit};

use super::{RuntimeCursor, Window};

impl RuntimeCursor<'_> {
    fn read_window<T: MemoryInt>(
        &self,
        body: &mut FunctionBuilder<'_>,
        window: &Window,
    ) -> Result<Val<T>, BuildError> {
        match self.fixed_offset {
            Some(offset) => self.fetch.memory.load(body, &window.physical_start, offset),
            None => self
                .fetch
                .memory
                .load(body, &window.physical_start.add(&self.offset), 0),
        }
    }

    pub(in super::super) fn byte(
        &mut self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<Val<I8>, BuildError> {
        if self.maximum_offset >= MAX_INSTRUCTION_BYTES {
            body.if_(
                self.offset.unsigned().ge(MAX_INSTRUCTION_BYTES),
                |limit_body| {
                    exit::exception(
                        limit_body,
                        Exception::GeneralProtection {
                            error_code: 0.into(),
                        },
                    )
                },
            )?;
        }
        let value = match &self.window {
            Some(window) if window.covers(self.maximum_offset, 1) => {
                self.read_window(body, window)?
            }
            _ => self.fetch.byte(body, &self.next_eip())?,
        };
        self.advance(1);
        Ok(value)
    }

    pub(in super::super) fn dword(
        &mut self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<Val<I32>, BuildError> {
        self.read(body)
    }

    pub(super) fn read<T: MemoryInt>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<Val<T>, BuildError>
    where
        I32: AtLeast<T>,
    {
        if let Some(window) = &self.window {
            if window.covers(self.maximum_offset, T::BYTES) {
                let value = self.read_window(body, window)?;
                self.advance(T::BYTES);
                return Ok(value);
            }
        }
        let direct = self
            .fetch
            .check_direct_access(body, &self.next_eip(), T::BYTES)?;
        let needs_byte_reads = if self.maximum_offset + T::BYTES > MAX_INSTRUCTION_BYTES {
            direct.unavailable.or(self
                .offset
                .unsigned()
                .ge(MAX_INSTRUCTION_BYTES - T::BYTES + 1))
        } else {
            direct.unavailable
        };
        let value = body.if_value::<T>(
            needs_byte_reads,
            |mut field_body| {
                // Retry required bytes in order. An earlier missing page faults
                // before a later CS or instruction-length violation. EIP may wrap.
                let mut field_cursor = self.clone();
                let mut value = field_cursor
                    .byte(&mut field_body)?
                    .unsigned()
                    .extend::<I32>();
                for offset in 1..T::BYTES {
                    let next = field_cursor.byte(&mut field_body)?;
                    value = value.or(next.unsigned().extend::<I32>().shl(offset * 8));
                }
                field_body.yield_(value.truncate::<T>())
            },
            |mut field_body| {
                let value = self
                    .fetch
                    .memory
                    .load::<T>(&mut field_body, &direct.physical, 0)?;
                field_body.yield_(value)
            },
        )?;
        self.advance(T::BYTES);
        Ok(value)
    }
}
