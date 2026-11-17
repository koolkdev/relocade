//! Decoder entry signatures and transport of cursor progress and semantic state.
use std::sync::Arc;

use relocade_compiler::{
    Argument, BlockBuilder, BuildError, Func, Program, Signature, Type, Val, I1, I32,
};

use crate::{
    decode::DecodeState,
    instruction::{FormSelection, OpcodeMap, PrefixState, SegmentOverride},
};

use super::{cursor::RuntimeCursor, RuntimeDecoder};

/// How a private entry receives the code default captured by its caller.
#[derive(Clone, Copy, Eq, PartialEq)]
enum CodeDefault {
    Bits16,
    Bits32,
    Parameter,
}

impl CodeDefault {
    const ALL: [Self; 3] = [Self::Bits16, Self::Bits32, Self::Parameter];

    fn for_value(body: &BlockBuilder<'_>, value: &Val<I1>) -> Result<Self, BuildError> {
        Ok(match body.constant_bits(value)? {
            Some(0) => Self::Bits16,
            Some(_) => Self::Bits32,
            None => Self::Parameter,
        })
    }

    fn value(self, body: &BlockBuilder<'_>, parameter: u32) -> Result<Val<I1>, BuildError> {
        match self {
            Self::Bits16 => body.value(false),
            Self::Bits32 => body.value(true),
            Self::Parameter => body.parameter(parameter),
        }
    }
}

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

    fn field_types(self) -> &'static [Type] {
        match self {
            Self::Opcode => &[Type::I8],
            Self::MemoryOperand(_) => &[Type::I32, Type::I8],
        }
    }

    pub(super) fn consumed(self) -> u32 {
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
    default_selection: FormSelection,
    direct: Option<[Func; 3]>,
    checked: [Func; 3],
    resumed: Vec<ResumedEntry>,
}

#[derive(Clone)]
struct ResumedEntry {
    selection: FormSelection,
    has_segment_override: bool,
    functions: [Func; 3],
}

impl DecodeHandlers {
    pub(super) fn declare(program: &mut Program, point: DecodePoint, direct_entry: bool) -> Self {
        let default_selection = FormSelection::default();
        let mut declare = |code_default, cursor_parameters: &[Type]| {
            let mut parameters = vec![Type::I1, Type::I32];
            parameters.extend_from_slice(point.field_types());
            if code_default == CodeDefault::Parameter {
                parameters.push(Type::I1);
            }
            parameters.extend_from_slice(cursor_parameters);
            program.declare(Signature {
                parameters,
                results: crate::abi::RUN.signature().results,
            })
        };
        // Calls select a variant using compiler facts. Only reachable variants
        // build bodies, and a known default needs no argument.
        let checked = CodeDefault::ALL.map(|default| declare(default, &[]));
        let direct =
            direct_entry.then(|| CodeDefault::ALL.map(|default| declare(default, &[Type::I32])));
        // Every resumed entry receives cursor progress. Only entries reached
        // through a segment prefix receive an override index as well.
        let mut resumed = Vec::new();
        for has_segment_override in [false, true] {
            let cursor_parameters = if has_segment_override {
                &[Type::I32, Type::I32][..]
            } else {
                &[Type::I32][..]
            };
            for selection in FormSelection::combinations().filter(|selection| {
                point.accepts(&selection.prefixes())
                    && (has_segment_override || *selection != default_selection)
            }) {
                let functions = CodeDefault::ALL.map(|default| declare(default, cursor_parameters));
                resumed.push(ResumedEntry {
                    selection,
                    has_segment_override,
                    functions,
                });
            }
        }
        Self {
            point,
            default_selection,
            direct,
            checked,
            resumed,
        }
    }

    pub(super) fn define(
        &self,
        program: &mut Program,
        decoder: Arc<RuntimeDecoder>,
        decode: impl Fn(
                &RuntimeDecoder,
                BlockBuilder<'_>,
                RuntimeCursor<'_>,
                DecodeState,
                Val<I1>,
            ) -> Result<(), BuildError>
            + Send
            + Sync
            + 'static,
    ) -> Result<(), BuildError> {
        #[derive(Clone)]
        enum Entry {
            Checked,
            Direct,
            Resumed(ResumedEntry),
        }
        let entries = [(self.checked, Entry::Checked)]
            .into_iter()
            .chain(self.direct.map(|functions| (functions, Entry::Direct)))
            .chain(
                self.resumed
                    .iter()
                    .map(|entry| (entry.functions, Entry::Resumed(entry.clone()))),
            )
            .flat_map(|(functions, entry)| {
                CodeDefault::ALL.map(|code_default| {
                    (
                        functions[code_default as usize],
                        entry.clone(),
                        code_default,
                    )
                })
            });
        let decode = Arc::new(decode);
        for (function, entry, code_default) in entries {
            let decoder = decoder.clone();
            let decode = decode.clone();
            let point = self.point;
            let default_selection = self.default_selection;
            program.define(function, move |body| {
                let fetch = decoder.fetch();
                let instruction_eip = body.parameter::<I32>(1)?;
                let default_parameter = point.field_types().len() as u32 + 2;
                let code_default_is_32_bit = code_default.value(&body, default_parameter)?;
                let position_parameter =
                    default_parameter + u32::from(code_default == CodeDefault::Parameter);
                let (cursor, prefixes) = match entry {
                    Entry::Resumed(entry) => {
                        let cursor = RuntimeCursor::resume(
                            fetch,
                            &instruction_eip,
                            &body.parameter::<I32>(position_parameter)?,
                        );
                        let mut prefixes = entry.selection.prefixes();
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
                            point.consumed(),
                        )?;
                        (cursor, default_selection.prefixes())
                    }
                };
                decode(
                    &decoder,
                    body,
                    cursor,
                    point.state(prefixes),
                    code_default_is_32_bit,
                )
            })?;
        }
        Ok(())
    }

    /// All entries carry the single-step flag first, followed by instruction EIP.
    /// Opcode entries receive a byte; memory entries receive a validated form
    /// index and ModRM. Live CS.D, cursor progress and optional prefix arguments
    /// follow these decoded fields.
    pub(super) fn tail_call(
        &self,
        body: BlockBuilder<'_>,
        cursor: &RuntimeCursor<'_>,
        state: DecodeState,
        fields: &[Argument],
        code_default_is_32_bit: &Val<I1>,
    ) -> Result<(), BuildError> {
        let code_default = CodeDefault::for_value(&body, code_default_is_32_bit)?;
        let variant = code_default as usize;
        let single_step = body.parameter::<I1>(0)?;
        let mut arguments = vec![(&single_step).into(), cursor.instruction_eip().into()];
        arguments.extend_from_slice(fields);
        if code_default == CodeDefault::Parameter {
            arguments.push(code_default_is_32_bit.into());
        }
        let target = if cursor.fixed_offset() == Some(self.point.consumed()) {
            match cursor.physical_start() {
                Some(physical_start) => {
                    arguments.push(physical_start.into());
                    self.direct.expect("direct decoding has a function entry")[variant]
                }
                None => self.checked[variant],
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
                        && entry.selection == *state.prefixes.selection()
                })
                .map(|entry| entry.functions[variant])
                .expect("the prefix state has a viable decoder entry")
        };
        body.tail_call(target, &arguments)
    }
}
