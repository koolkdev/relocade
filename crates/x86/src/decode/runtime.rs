mod cursor;

use wasm86_compiler::{BuildError, Func, FunctionBuilder, Program, Signature, Type, Val, I32, I8};

use crate::{
    address::{Address32, IndexTerm, RegisterTerm},
    instruction::{
        DecodedFields, DecodedInstruction, Encoding, Form, Location, ACCUMULATOR_OFFSET_FORMS,
        MODRM_FORMS, MOV_BYTE_IMMEDIATE, MOV_DWORD_IMMEDIATE,
    },
    memory::{DirectRange, Intent, Memory},
    register::{Register, RegisterCode},
    state::exit,
};

use self::cursor::RuntimeCursor;

/// Builds decoding code that reads guest instruction bytes during execution.
/// Separate handlers keep the entry graph small to discourage V8 from inlining
/// large checked-fetch code into the common path and adding stack spills.
/// ModRM layouts also get separate entries so opcode-extension checks do not
/// burden register/r/m moves or prevent their handlers from inlining.
pub(crate) struct RuntimeDecoder<C> {
    memory: Memory,
    register_rm_handlers: OperandHandlers,
    rm_immediate_handlers: OperandHandlers,
    absolute_offset_handlers: OperandHandlers,
    modrm_memory_handler: Func,
    complete_instruction: C,
}

impl<C> RuntimeDecoder<C>
where
    C: Fn(FunctionBuilder<'_>, DecodedInstruction<Val<I32>, Val<I32>>) -> Result<(), BuildError>,
{
    pub(crate) fn new(
        program: &mut Program,
        memory: Memory,
        complete_instruction: C,
    ) -> Result<Self, BuildError> {
        let register_rm_handlers = OperandHandlers::declare(program);
        let modrm_memory_handler = program.declare(Signature {
            parameters: vec![Type::I32, Type::I8, Type::I8],
            result: Type::I64,
        });
        let decoder = Self {
            memory,
            register_rm_handlers,
            rm_immediate_handlers: OperandHandlers::declare(program),
            absolute_offset_handlers: OperandHandlers::declare(program),
            modrm_memory_handler,
            complete_instruction,
        };

        let body = program.define(modrm_memory_handler)?;
        let instruction_eip = body.parameter::<I32>(0)?;
        let opcode = body.parameter::<I8>(1)?;
        let modrm = body.parameter::<I8>(2)?;
        let cursor = RuntimeCursor::new(
            &body,
            memory,
            &instruction_eip,
            None,
            Encoding::OPCODE_BYTES + 1,
        )?;
        decoder.decode_modrm_memory(body, cursor, &opcode, &modrm)?;

        decoder
            .register_rm_handlers
            .define(program, memory, |body, cursor, opcode| {
                decoder.decode_modrm(
                    body,
                    cursor,
                    opcode,
                    MODRM_FORMS
                        .iter()
                        .filter(|form| matches!(form.encoding, Encoding::RegisterRm { .. })),
                )
            })?;
        decoder
            .rm_immediate_handlers
            .define(program, memory, |body, cursor, opcode| {
                decoder.decode_modrm(
                    body,
                    cursor,
                    opcode,
                    MODRM_FORMS
                        .iter()
                        .filter(|form| matches!(form.encoding, Encoding::RmImmediate { .. })),
                )
            })?;
        decoder
            .absolute_offset_handlers
            .define(program, memory, |body, cursor, opcode| {
                decoder.decode_absolute_offset(body, cursor, opcode)
            })?;
        Ok(decoder)
    }

    pub(crate) fn direct_window(
        &self,
        body: &mut FunctionBuilder<'_>,
        instruction_eip: &Val<I32>,
    ) -> Result<DirectRange, BuildError> {
        // Fields beyond this window use checked fetch. Extending the common
        // proof for longer forms would burden shorter instructions.
        let bytes = MOV_DWORD_IMMEDIATE.minimum_length();
        self.memory
            .check_direct_access(body, instruction_eip, bytes, Intent::Fetch)
    }

    /// `physical_start` supplies the proven contiguous instruction window. The stored
    /// completion policy consumes each selected path and its decoded operands.
    pub(crate) fn decode(
        &self,
        mut body: FunctionBuilder<'_>,
        instruction_eip: &Val<I32>,
        physical_start: Option<&Val<I32>>,
    ) -> Result<(), BuildError> {
        let mut cursor =
            RuntimeCursor::new(&body, self.memory, instruction_eip, physical_start, 0)?;
        let opcode = cursor.byte(&mut body)?;
        body.if_(MOV_DWORD_IMMEDIATE.matches_value(&opcode), |form_body| {
            self.decode_immediate(form_body, cursor.clone(), &opcode, &MOV_DWORD_IMMEDIATE)
        })?;
        for form in &MODRM_FORMS {
            let handlers = match form.encoding {
                Encoding::RegisterRm { .. } => &self.register_rm_handlers,
                Encoding::RmImmediate { .. } => &self.rm_immediate_handlers,
                _ => unreachable!("the selected form has a ModRM field"),
            };
            body.if_(form.matches_value(&opcode), |form_body| {
                handlers.tail_call(form_body, instruction_eip, &opcode, physical_start)
            })?;
        }
        body.if_(MOV_BYTE_IMMEDIATE.matches_value(&opcode), |form_body| {
            self.decode_immediate(form_body, cursor.clone(), &opcode, &MOV_BYTE_IMMEDIATE)
        })?;
        for form in &ACCUMULATOR_OFFSET_FORMS {
            body.if_(form.matches_value(&opcode), |form_body| {
                self.absolute_offset_handlers.tail_call(
                    form_body,
                    instruction_eip,
                    &opcode,
                    physical_start,
                )
            })?;
        }
        body.return_(exit::unsupported(instruction_eip, &opcode))
    }

    fn decode_immediate(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor,
        opcode: &Val<I8>,
        form: &Form,
    ) -> Result<(), BuildError> {
        let bits = cursor.immediate(&mut body, form.width)?;
        let register = RegisterCode::indexed(opcode.unsigned().extend::<I32>());
        let decoded_instruction = form.bind(
            DecodedFields::OpcodeRegisterImmediate {
                register,
                immediate: bits,
            },
            cursor.instruction_eip().clone(),
            cursor.next_eip(),
        );
        (self.complete_instruction)(body, decoded_instruction)
    }

    fn decode_absolute_offset(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor,
        opcode: &Val<I8>,
    ) -> Result<(), BuildError> {
        let offset = cursor.dword(&mut body)?;
        for form in &ACCUMULATOR_OFFSET_FORMS {
            body.if_(form.matches_value(opcode), |form_body| {
                let instruction = form.bind(
                    DecodedFields::AccumulatorOffset {
                        offset: offset.clone(),
                    },
                    cursor.instruction_eip().clone(),
                    cursor.next_eip(),
                );
                (self.complete_instruction)(form_body, instruction)
            })?;
        }
        body.return_(exit::unsupported(cursor.instruction_eip(), opcode))
    }

    fn decode_modrm(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor,
        opcode: &Val<I8>,
        forms: impl Iterator<Item = &'static Form> + Clone,
    ) -> Result<(), BuildError> {
        // The entry has selected a ModRM form. Read its shared encoding before
        // binding the register and r/m fields to their semantic roles.
        let modrm = cursor.byte(&mut body)?;
        for form in forms.clone() {
            if let Some(mismatch) = form.extension_mismatch(&modrm) {
                body.if_(
                    form.matches_value(opcode).and(mismatch),
                    |unsupported_body| {
                        unsupported_body
                            .return_(exit::unsupported(cursor.instruction_eip(), opcode))
                    },
                )?;
            }
        }
        body.if_(modrm.unsigned().shr(6).ne(3), |memory_operand_body| {
            memory_operand_body.tail_call(
                self.modrm_memory_handler,
                &[
                    cursor.instruction_eip().into(),
                    opcode.into(),
                    (&modrm).into(),
                ],
            )
        })?;
        for form in forms {
            body.if_(form.matches_value(opcode), |mut form_body| {
                let mut form_cursor = cursor.clone();
                let rm =
                    Location::Register(RegisterCode::indexed(modrm.unsigned().extend::<I32>()));
                let fields = form_cursor.modrm_fields(&mut form_body, form, &modrm, rm)?;
                let instruction = form.bind(
                    fields,
                    form_cursor.instruction_eip().clone(),
                    form_cursor.next_eip(),
                );
                (self.complete_instruction)(form_body, instruction)
            })?;
        }
        body.return_(exit::unsupported(cursor.instruction_eip(), opcode))
    }

    fn decode_modrm_memory(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor,
        opcode: &Val<I8>,
        modrm: &Val<I8>,
    ) -> Result<(), BuildError> {
        let mode = modrm.unsigned().shr(6);
        let rm = modrm.and(7).unsigned().extend::<I32>();
        let has_sib = rm.eq(4);
        let sib = cursor.optional_byte(&mut body, &has_sib)?;
        let base = has_sib.select(sib.and(7).unsigned().extend::<I32>(), &rm);
        let no_base = mode.eq(0).and(base.eq(5));
        let displacement = cursor.displacement(&mut body, &mode, &no_base)?;
        for form in &MODRM_FORMS {
            body.if_(form.matches_value(opcode), |mut form_body| {
                let address = Address32 {
                    base: Some(RegisterTerm {
                        register: Register::<I32>::indexed(base.clone()),
                        present: Some(no_base.eq(0)),
                    }),
                    index: Some(IndexTerm {
                        register: RegisterTerm {
                            register: Register::<I32>::indexed(
                                sib.unsigned().shr(3).unsigned().extend::<I32>(),
                            ),
                            present: Some(has_sib.and(sib.unsigned().shr(3).and(7).ne(4))),
                        },
                        shift: sib.unsigned().shr(6).unsigned().extend::<I32>(),
                    }),
                    displacement: displacement.clone(),
                };
                let mut form_cursor = cursor.clone();
                let fields = form_cursor.modrm_fields(
                    &mut form_body,
                    form,
                    modrm,
                    Location::Memory(address),
                )?;
                let instruction = form.bind(
                    fields,
                    form_cursor.instruction_eip().clone(),
                    form_cursor.next_eip(),
                );
                (self.complete_instruction)(form_body, instruction)
            })?;
        }
        body.return_(exit::unsupported(cursor.instruction_eip(), opcode))
    }
}

/// A selected operand layout has two fetch entries: one continues the proven
/// instruction window, and the other checks each read. Both use the same decoder.
struct OperandHandlers {
    direct: Func,
    checked: Func,
}

impl OperandHandlers {
    fn declare(program: &mut Program) -> Self {
        Self {
            checked: program.declare(Signature {
                parameters: vec![Type::I32, Type::I8],
                result: Type::I64,
            }),
            direct: program.declare(Signature {
                parameters: vec![Type::I32, Type::I8, Type::I32],
                result: Type::I64,
            }),
        }
    }

    fn define(
        &self,
        program: &mut Program,
        memory: Memory,
        decode: impl Fn(FunctionBuilder<'_>, RuntimeCursor, &Val<I8>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        for (function, has_window) in [(self.checked, false), (self.direct, true)] {
            let body = program.define(function)?;
            let instruction_eip = body.parameter::<I32>(0)?;
            let opcode = body.parameter::<I8>(1)?;
            let physical_start = if has_window {
                Some(body.parameter::<I32>(2)?)
            } else {
                None
            };
            let cursor = RuntimeCursor::new(
                &body,
                memory,
                &instruction_eip,
                physical_start.as_ref(),
                Encoding::OPCODE_BYTES,
            )?;
            decode(body, cursor, &opcode)?;
        }
        Ok(())
    }

    fn tail_call(
        &self,
        body: FunctionBuilder<'_>,
        instruction_eip: &Val<I32>,
        opcode: &Val<I8>,
        physical_start: Option<&Val<I32>>,
    ) -> Result<(), BuildError> {
        match physical_start {
            Some(physical_start) => body.tail_call(
                self.direct,
                &[instruction_eip.into(), opcode.into(), physical_start.into()],
            ),
            None => body.tail_call(self.checked, &[instruction_eip.into(), opcode.into()]),
        }
    }
}
