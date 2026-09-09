mod address;
mod cursor;
mod handlers;
mod operands;
mod selectors;

use wasm86_compiler::{BuildError, FunctionBuilder, Program, Val, I32, I8};

use crate::{
    instruction::{DecodedInstruction, OpcodeMap},
    memory::{DirectRange, Intent, Memory},
};

use self::{
    cursor::{RuntimeCursor, DIRECT_FETCH_BYTES},
    handlers::{DecodeHandlers, DecodePoint},
};

/// Builds decoding code that reads guest instruction bytes during execution.
/// All primary forms share opcode selection. Direct, checked and prefixed
/// entries carry the cursor's fetch guarantees; memory entries resume after
/// opcode and ModRM validation.
pub(crate) struct RuntimeDecoder<C> {
    memory: Memory,
    opcode_handlers: DecodeHandlers,
    primary_modrm_memory_handlers: DecodeHandlers,
    extended_modrm_memory_handlers: DecodeHandlers,
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
        let decoder = Self {
            memory,
            opcode_handlers: DecodeHandlers::declare(program, DecodePoint::Opcode),
            primary_modrm_memory_handlers: DecodeHandlers::declare(
                program,
                DecodePoint::MemoryOperand(OpcodeMap::Primary),
            ),
            extended_modrm_memory_handlers: DecodeHandlers::declare(
                program,
                DecodePoint::MemoryOperand(OpcodeMap::Extended),
            ),
            complete_instruction,
        };
        for handlers in [
            &decoder.primary_modrm_memory_handlers,
            &decoder.extended_modrm_memory_handlers,
        ] {
            handlers.define(program, memory, |body, cursor, opcode| {
                let modrm = body.parameter::<I8>(2)?;
                decoder.decode_memory_operands(body, cursor, opcode, &modrm)
            })?;
        }

        decoder
            .opcode_handlers
            .define(program, memory, |body, cursor, opcode| {
                decoder.decode_opcode(body, cursor, opcode)
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
        self.memory
            .check_direct_access(body, instruction_eip, DIRECT_FETCH_BYTES, Intent::Fetch)
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
        self.opcode_handlers
            .tail_call(body, &cursor, &[(&opcode).into()])
    }
}
