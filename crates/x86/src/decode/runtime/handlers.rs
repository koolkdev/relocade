use wasm86_compiler::{
    Argument, BuildError, Func, FunctionBuilder, Program, Signature, Type, Val, I32, I8,
};

use crate::{
    instruction::{OpcodeMap, OperandSize},
    memory::Memory,
};

use super::cursor::RuntimeCursor;

/// Identifies the already-decoded fields supplied to a generated decoder entry.
#[derive(Clone, Copy)]
pub(super) enum DecodePoint {
    Opcode,
    MemoryOperand(OpcodeMap),
}

impl DecodePoint {
    fn opcode_map(self) -> OpcodeMap {
        match self {
            Self::Opcode => OpcodeMap::Primary,
            Self::MemoryOperand(map) => map,
        }
    }

    fn field_count(self) -> usize {
        match self {
            Self::Opcode => 1,
            Self::MemoryOperand(_) => 2,
        }
    }

    fn consumed(self) -> u32 {
        self.opcode_map().bytes() + u32::from(matches!(self, Self::MemoryOperand(_)))
    }
}

/// Direct and checked entries share field decoding. A prefixed entry resumes
/// from the instruction's total byte count. Passing the proven physical window
/// through a direct entry lets later operand fields reuse the original check.
pub(super) struct DecodeHandlers {
    point: DecodePoint,
    direct: Func,
    checked: Func,
    prefixed: Func,
    repeated: Option<[Func; 2]>,
}

impl DecodeHandlers {
    pub(super) fn declare(program: &mut Program, point: DecodePoint) -> Self {
        let mut parameters = vec![Type::I32];
        parameters.resize(point.field_count() + 1, Type::I8);
        let checked = program.declare(Signature {
            parameters: parameters.clone(),
            results: vec![Type::I64],
        });
        parameters.push(Type::I32);
        Self {
            point,
            checked,
            direct: program.declare(Signature {
                parameters: parameters.clone(),
                results: vec![Type::I64],
            }),
            prefixed: program.declare(Signature {
                parameters: parameters.clone(),
                results: vec![Type::I64],
            }),
            // Repeat forms have implicit operands, so only opcode entries need
            // these restricted selectors. Ordinary operand decoders stay shared.
            repeated: matches!(point, DecodePoint::Opcode).then(|| {
                std::array::from_fn(|_| {
                    program.declare(Signature {
                        parameters: parameters.clone(),
                        results: vec![Type::I64],
                    })
                })
            }),
        }
    }

    pub(super) fn define<'memory>(
        &self,
        program: &mut Program,
        memory: &'memory Memory,
        decode: impl Fn(FunctionBuilder<'_>, RuntimeCursor<'memory>, &Val<I8>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        enum Entry {
            Checked,
            Direct,
            Prefixed(OperandSize, bool),
        }
        let mut entries = vec![
            (self.checked, Entry::Checked),
            (self.direct, Entry::Direct),
            (self.prefixed, Entry::Prefixed(OperandSize::Word, false)),
        ];
        if let Some([dword, word]) = self.repeated {
            entries.push((dword, Entry::Prefixed(OperandSize::Dword, true)));
            entries.push((word, Entry::Prefixed(OperandSize::Word, true)));
        }
        for (function, entry) in entries {
            let body = program.define(function)?;
            let instruction_eip = body.parameter::<I32>(0)?;
            let opcode = body.parameter::<I8>(1)?;
            let position_parameter = self.point.field_count() as u32 + 1;
            let mut cursor = if let Entry::Prefixed(size, repeat) = entry {
                let mut cursor = RuntimeCursor::resume(
                    memory,
                    &instruction_eip,
                    &body.parameter::<I32>(position_parameter)?,
                    size,
                );
                if repeat {
                    cursor.select_repeat();
                }
                cursor
            } else {
                let physical_start = if matches!(entry, Entry::Direct) {
                    Some(body.parameter::<I32>(position_parameter)?)
                } else {
                    None
                };
                RuntimeCursor::new(
                    &body,
                    memory,
                    &instruction_eip,
                    physical_start.as_ref(),
                    self.point.consumed(),
                )?
            };
            if self.point.opcode_map() == OpcodeMap::Extended {
                cursor.enter_extended_map();
            }
            decode(body, cursor, &opcode)?;
        }
        Ok(())
    }

    /// `fields` contains the opcode, followed by ModRM at a memory-operand entry.
    pub(super) fn tail_call(
        &self,
        body: FunctionBuilder<'_>,
        cursor: &RuntimeCursor<'_>,
        fields: &[Argument],
    ) -> Result<(), BuildError> {
        let mut arguments = vec![cursor.instruction_eip().into()];
        arguments.extend_from_slice(fields);
        let target = if cursor.repeat_prefix() {
            arguments.push(cursor.consumed().into());
            let [dword, word] = self.repeated.expect("repeat forms use the opcode entry");
            match cursor.operand_size() {
                OperandSize::Word => word,
                OperandSize::Dword => dword,
            }
        } else {
            match cursor.operand_size() {
                OperandSize::Word => {
                    arguments.push(cursor.consumed().into());
                    self.prefixed
                }
                OperandSize::Dword => match cursor.physical_start() {
                    Some(physical_start) => {
                        arguments.push(physical_start.into());
                        self.direct
                    }
                    None => self.checked,
                },
            }
        };
        body.tail_call(target, &arguments)
    }
}
