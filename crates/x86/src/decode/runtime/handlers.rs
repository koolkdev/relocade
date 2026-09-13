//! Decoder entry signatures and transport of cursor progress and semantic state.
use wasm86_compiler::{
    Argument, BuildError, Func, FunctionBuilder, Program, Signature, Type, Val, I32, I8,
};

use crate::{
    decode::DecodeState,
    instruction::{OpcodeMap, PrefixState, SegmentOverride},
};

use super::{cursor::RuntimeCursor, InstructionFetch};

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

    fn accepts(self, prefixes: &PrefixState) -> bool {
        self.state(prefixes.clone()).forms().any(|form| match self {
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
    resumed: Vec<ResumedEntry>,
}

struct ResumedEntry {
    prefixes: PrefixState,
    has_segment_override: bool,
    function: Func,
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
        // Every resumed entry receives cursor progress. Only entries reached
        // through a segment prefix receive an override index as well.
        let mut resumed = Vec::new();
        for has_segment_override in [false, true] {
            let mut parameters = parameters.clone();
            if has_segment_override {
                parameters.push(Type::I32);
            }
            for prefixes in std::iter::once(PrefixState::default())
                .chain(PrefixState::PREFIXED)
                .filter(|prefixes| {
                    point.accepts(prefixes)
                        && (has_segment_override
                            || !prefixes.same_form_selection(&PrefixState::default()))
                })
            {
                let function = program.declare(Signature {
                    parameters: parameters.clone(),
                    results: vec![Type::I64],
                });
                resumed.push(ResumedEntry {
                    prefixes,
                    has_segment_override,
                    function,
                });
            }
        }
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
        fetch: InstructionFetch<'memory>,
        decode: impl Fn(
            FunctionBuilder<'_>,
            RuntimeCursor<'memory>,
            DecodeState,
            &Val<I8>,
        ) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        enum Entry<'entry> {
            Checked,
            Direct,
            Resumed(&'entry ResumedEntry),
        }
        let entries = [(self.checked, Entry::Checked), (self.direct, Entry::Direct)]
            .into_iter()
            .chain(
                self.resumed
                    .iter()
                    .map(|entry| (entry.function, Entry::Resumed(entry))),
            );
        for (function, entry) in entries {
            let body = program.define(function)?;
            let instruction_eip = body.parameter::<I32>(0)?;
            let opcode = body.parameter::<I8>(1)?;
            let position_parameter = self.point.field_count() as u32 + 1;
            let (cursor, prefixes) = match entry {
                Entry::Resumed(entry) => {
                    let cursor = RuntimeCursor::resume(
                        fetch,
                        &instruction_eip,
                        &body.parameter::<I32>(position_parameter)?,
                    );
                    let mut prefixes = entry.prefixes.clone();
                    if entry.has_segment_override {
                        prefixes = prefixes.with_segment_override(SegmentOverride::Runtime(
                            body.parameter::<I32>(position_parameter + 1)?,
                        ));
                    }
                    (cursor, prefixes)
                }
                Entry::Checked | Entry::Direct => {
                    let physical_start = if matches!(entry, Entry::Direct) {
                        Some(body.parameter::<I32>(position_parameter)?)
                    } else {
                        None
                    };
                    let cursor = RuntimeCursor::new(
                        &body,
                        fetch,
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
            let segment_override = state.prefixes.segment_override().index();
            if let Some(index) = &segment_override {
                arguments.push(index.into());
            }
            self.resumed
                .iter()
                .find(|entry| {
                    entry.has_segment_override == segment_override.is_some()
                        && entry.prefixes.same_form_selection(&state.prefixes)
                })
                .map(|entry| entry.function)
                .expect("the prefix state has a viable decoder entry")
        };
        body.tail_call(target, &arguments)
    }
}
