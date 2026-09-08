mod cursor;
mod handlers;
mod operands;

use wasm86_compiler::{BuildError, Func, FunctionBuilder, Program, Signature, Type, Val, I32, I8};

use crate::{
    instruction::{
        DecodedInstruction, Encoding, OperandSize, ACCUMULATOR_OFFSET_FORMS, MODRM_FORMS,
        MOV_BYTE_IMMEDIATE, MOV_OPERAND_IMMEDIATE, OPERAND_SIZE_PREFIX,
    },
    memory::{DirectRange, Intent, Memory},
    state::exit,
};

use self::{cursor::RuntimeCursor, handlers::OperandHandlers};

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
    prefixed_modrm_memory_handler: Func,
    operand_prefix_handler: Func,
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
            prefixed_modrm_memory_handler: program.declare(Signature {
                parameters: vec![Type::I32, Type::I8, Type::I8, Type::I32],
                result: Type::I64,
            }),
            operand_prefix_handler: program.declare(Signature {
                parameters: vec![Type::I32, Type::I8, Type::I32],
                result: Type::I64,
            }),
            complete_instruction,
        };

        for (handler, prefixed) in [
            (decoder.modrm_memory_handler, false),
            (decoder.prefixed_modrm_memory_handler, true),
        ] {
            let body = program.define(handler)?;
            let instruction_eip = body.parameter::<I32>(0)?;
            let opcode = body.parameter::<I8>(1)?;
            let modrm = body.parameter::<I8>(2)?;
            let cursor = if prefixed {
                RuntimeCursor::after_prefix(memory, &instruction_eip, &body.parameter::<I32>(3)?)
            } else {
                RuntimeCursor::new(
                    &body,
                    memory,
                    &instruction_eip,
                    None,
                    Encoding::OPCODE_BYTES + 1,
                )?
            };
            decoder.decode_modrm_memory(body, cursor, &opcode, &modrm)?;
        }

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
        let mut body = program.define(decoder.operand_prefix_handler)?;
        let instruction_eip = body.parameter::<I32>(0)?;
        let opcode = body.parameter::<I8>(1)?;
        let consumed = body.parameter::<I32>(2)?;
        body.if_(
            opcode.ne(u32::from(OPERAND_SIZE_PREFIX)),
            |unsupported_body| {
                unsupported_body.return_(exit::unsupported(&instruction_eip, &opcode))
            },
        )?;
        let mut cursor = RuntimeCursor::after_prefix(memory, &instruction_eip, &consumed);
        let opcode = cursor.byte(&mut body)?;
        decoder.decode_opcode(body, cursor, &opcode)?;
        Ok(decoder)
    }

    pub(crate) fn direct_window(
        &self,
        body: &mut FunctionBuilder<'_>,
        instruction_eip: &Val<I32>,
    ) -> Result<DirectRange, BuildError> {
        // Fields beyond this window use checked fetch. Extending the common
        // proof for longer forms would burden shorter instructions.
        let bytes = MOV_OPERAND_IMMEDIATE
            .resolve(OperandSize::Dword)
            .minimum_length();
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
        self.decode_opcode(body, cursor, &opcode)
    }

    fn decode_opcode(
        &self,
        mut body: FunctionBuilder<'_>,
        cursor: RuntimeCursor,
        opcode: &Val<I8>,
    ) -> Result<(), BuildError> {
        body.if_(MOV_OPERAND_IMMEDIATE.matches_value(opcode), |form_body| {
            self.decode_immediate(
                form_body,
                cursor.clone(),
                opcode,
                &MOV_OPERAND_IMMEDIATE.resolve(cursor.operand_size()),
            )
        })?;
        for form in &MODRM_FORMS {
            let handlers = match form.encoding {
                Encoding::RegisterRm { .. } => &self.register_rm_handlers,
                Encoding::RmImmediate { .. } => &self.rm_immediate_handlers,
                _ => unreachable!("the selected form has a ModRM field"),
            };
            body.if_(form.matches_value(opcode), |form_body| {
                handlers.tail_call(form_body, &cursor, opcode)
            })?;
        }
        body.if_(MOV_BYTE_IMMEDIATE.matches_value(opcode), |form_body| {
            self.decode_immediate(
                form_body,
                cursor.clone(),
                opcode,
                &MOV_BYTE_IMMEDIATE.resolve(cursor.operand_size()),
            )
        })?;
        for form in &ACCUMULATOR_OFFSET_FORMS {
            body.if_(form.matches_value(opcode), |form_body| {
                self.absolute_offset_handlers
                    .tail_call(form_body, &cursor, opcode)
            })?;
        }
        // The remaining selector is a prefix or an unsupported instruction.
        // Keeping that policy in the prefix reader leaves V8's inline budget
        // available for ordinary register moves in the common entry.
        body.tail_call(
            self.operand_prefix_handler,
            &[
                cursor.instruction_eip().into(),
                opcode.into(),
                cursor.consumed().into(),
            ],
        )
    }
}
