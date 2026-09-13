//! Decoder entry signatures and transport of cursor progress and semantic state.
use wasm86_compiler::{
    Argument, BuildError, Func, FunctionBuilder, Program, Signature, Type, Val, I32, I8,
};

use crate::{
    decode::DecodeState,
    instruction::{OpcodeMap, PrefixState},
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
    fn state(self, prefixes: PrefixState) -> DecodeState {
        DecodeState {
            prefixes,
            map: match self {
                Self::Opcode => OpcodeMap::Primary,
                Self::MemoryOperand(map) => map,
            },
        }
    }

    fn field_count(self) -> usize {
        match self {
            Self::Opcode => 1,
            Self::MemoryOperand(_) => 2,
        }
    }

    fn consumed(self) -> u32 {
        match self {
            Self::Opcode => OpcodeMap::Primary.bytes(),
            Self::MemoryOperand(map) => map.bytes() + 1,
        }
    }

    fn accepts(self, prefixes: PrefixState) -> bool {
        self.state(prefixes).forms().any(|form| match self {
            Self::Opcode => true,
            Self::MemoryOperand(_) => form.encoding.has_modrm(),
        })
    }
}

/// Fixed entries know the consumed field count; resumed entries receive it.
/// Semantic specialization is independent of the direct fetch-window proof.
pub(super) struct DecodeHandlers {
    point: DecodePoint,
    direct: Func,
    checked: Func,
    resumed: Vec<(PrefixState, Func)>,
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
        let direct = program.declare(Signature {
            parameters: parameters.clone(),
            results: vec![Type::I64],
        });
        let resumed = PrefixState::PREFIXED
            .into_iter()
            .filter(|&prefixes| point.accepts(prefixes))
            .map(|prefixes| {
                let function = program.declare(Signature {
                    parameters: parameters.clone(),
                    results: vec![Type::I64],
                });
                (prefixes, function)
            })
            .collect();
        Self {
            point,
            direct,
            checked,
            resumed,
        }
    }

    pub(super) fn define<'memory>(
        &self,
        program: &mut Program,
        memory: &'memory Memory,
        decode: impl Fn(
            FunctionBuilder<'_>,
            RuntimeCursor<'memory>,
            DecodeState,
            &Val<I8>,
        ) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        enum Entry {
            Checked,
            Direct,
            Resumed(PrefixState),
        }
        let entries = [(self.checked, Entry::Checked), (self.direct, Entry::Direct)]
            .into_iter()
            .chain(
                self.resumed
                    .iter()
                    .map(|&(prefixes, function)| (function, Entry::Resumed(prefixes))),
            );
        for (function, entry) in entries {
            let body = program.define(function)?;
            let instruction_eip = body.parameter::<I32>(0)?;
            let opcode = body.parameter::<I8>(1)?;
            let position_parameter = self.point.field_count() as u32 + 1;
            let (cursor, prefixes) = match entry {
                Entry::Resumed(prefixes) => (
                    RuntimeCursor::resume(
                        memory,
                        &instruction_eip,
                        &body.parameter::<I32>(position_parameter)?,
                    ),
                    prefixes,
                ),
                Entry::Checked | Entry::Direct => {
                    let physical_start = if matches!(entry, Entry::Direct) {
                        Some(body.parameter::<I32>(position_parameter)?)
                    } else {
                        None
                    };
                    let cursor = RuntimeCursor::new(
                        &body,
                        memory,
                        &instruction_eip,
                        physical_start.as_ref(),
                        self.point.consumed(),
                    )?;
                    (cursor, PrefixState::default())
                }
            };
            decode(body, cursor, self.point.state(prefixes), &opcode)?;
        }
        Ok(())
    }

    /// `fields` contains the opcode, followed by ModRM at a memory-operand entry.
    pub(super) fn tail_call(
        &self,
        body: FunctionBuilder<'_>,
        cursor: &RuntimeCursor<'_>,
        state: DecodeState,
        fields: &[Argument],
    ) -> Result<(), BuildError> {
        let mut arguments = vec![cursor.instruction_eip().into()];
        arguments.extend_from_slice(fields);
        let target = if cursor.fixed_offset() == Some(self.point.consumed()) {
            match cursor.physical_start() {
                Some(physical_start) => {
                    arguments.push(physical_start.into());
                    self.direct
                }
                None => self.checked,
            }
        } else {
            arguments.push(cursor.consumed().into());
            self.resumed
                .iter()
                .find(|(prefixes, _)| *prefixes == state.prefixes)
                .map(|(_, function)| *function)
                .expect("the prefix state has a viable decoder entry")
        };
        body.tail_call(target, &arguments)
    }
}
