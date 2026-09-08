use wasm86_compiler::{BuildError, Func, FunctionBuilder, Program, Signature, Type, Val, I32, I8};

use crate::{
    instruction::{Encoding, OperandSize},
    memory::Memory,
};

use super::cursor::RuntimeCursor;

/// Each operand layout continues either an unprefixed instruction window,
/// checked unprefixed reads, or the total byte count of a prefixed instruction.
/// All three entries use the same field decoder.
pub(super) struct OperandHandlers {
    direct: Func,
    checked: Func,
    prefixed: Func,
}

impl OperandHandlers {
    pub(super) fn declare(program: &mut Program) -> Self {
        Self {
            checked: program.declare(Signature {
                parameters: vec![Type::I32, Type::I8],
                result: Type::I64,
            }),
            direct: program.declare(Signature {
                parameters: vec![Type::I32, Type::I8, Type::I32],
                result: Type::I64,
            }),
            prefixed: program.declare(Signature {
                parameters: vec![Type::I32, Type::I8, Type::I32],
                result: Type::I64,
            }),
        }
    }

    pub(super) fn define(
        &self,
        program: &mut Program,
        memory: Memory,
        decode: impl Fn(FunctionBuilder<'_>, RuntimeCursor, &Val<I8>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        enum Entry {
            Checked,
            Direct,
            Prefixed,
        }
        for (function, entry) in [
            (self.checked, Entry::Checked),
            (self.direct, Entry::Direct),
            (self.prefixed, Entry::Prefixed),
        ] {
            let body = program.define(function)?;
            let instruction_eip = body.parameter::<I32>(0)?;
            let opcode = body.parameter::<I8>(1)?;
            let cursor = if matches!(entry, Entry::Prefixed) {
                RuntimeCursor::after_prefix(memory, &instruction_eip, &body.parameter::<I32>(2)?)
            } else {
                let physical_start = if matches!(entry, Entry::Direct) {
                    Some(body.parameter::<I32>(2)?)
                } else {
                    None
                };
                RuntimeCursor::new(
                    &body,
                    memory,
                    &instruction_eip,
                    physical_start.as_ref(),
                    Encoding::OPCODE_BYTES,
                )?
            };
            decode(body, cursor, &opcode)?;
        }
        Ok(())
    }

    pub(super) fn tail_call(
        &self,
        body: FunctionBuilder<'_>,
        cursor: &RuntimeCursor,
        opcode: &Val<I8>,
    ) -> Result<(), BuildError> {
        let instruction_eip = cursor.instruction_eip();
        match cursor.operand_size() {
            OperandSize::Word => body.tail_call(
                self.prefixed,
                &[
                    instruction_eip.into(),
                    opcode.into(),
                    cursor.consumed().into(),
                ],
            ),
            OperandSize::Dword => match cursor.physical_start() {
                Some(physical_start) => body.tail_call(
                    self.direct,
                    &[instruction_eip.into(), opcode.into(), physical_start.into()],
                ),
                None => body.tail_call(self.checked, &[instruction_eip.into(), opcode.into()]),
            },
        }
    }
}
