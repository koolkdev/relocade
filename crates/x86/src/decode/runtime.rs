mod address;
mod cursor;
mod fetch;
mod handlers;
mod operands;
mod selectors;

use wasm86_compiler::{BuildError, FunctionBuilder, Program, Val, I32, I8};

use crate::{
    instruction::{DecodedInstruction, OpcodeMap},
    memory::DirectRange,
    state::exit,
};

use super::DecodeState;

pub(crate) use fetch::InstructionFetch;

use self::{
    cursor::{RuntimeCursor, DIRECT_FETCH_BYTES},
    handlers::{DecodeHandlers, DecodePoint},
};

/// Builds decoding code that reads guest instruction bytes during execution.
/// All primary forms share opcode selection. Direct, checked and resumed
/// entries carry the cursor's fetch guarantees; memory entries resume after
/// opcode and ModRM validation.
pub(crate) struct RuntimeDecoder<'memory, C> {
    fetch: InstructionFetch<'memory>,
    opcode_handlers: DecodeHandlers,
    primary_modrm_memory_handlers: DecodeHandlers,
    extended_modrm_memory_handlers: DecodeHandlers,
    complete_instruction: C,
}

impl<'memory, C> RuntimeDecoder<'memory, C>
where
    C: Fn(FunctionBuilder<'_>, DecodedInstruction<Val<I32>, Val<I32>>) -> Result<(), BuildError>,
{
    pub(crate) fn new(
        program: &mut Program,
        fetch: InstructionFetch<'memory>,
        complete_instruction: C,
    ) -> Result<Self, BuildError> {
        let decoder = Self {
            fetch,
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
            handlers.define(program, fetch, |body, cursor, state, opcode| {
                let modrm = body.parameter::<I8>(2)?;
                decoder.decode_memory_operands(body, cursor, state, opcode, &modrm)
            })?;
        }

        decoder
            .opcode_handlers
            .define(program, fetch, |body, cursor, state, opcode| {
                decoder.decode_opcode(body, cursor, state, opcode)
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
        self.fetch
            .check_direct_access(body, instruction_eip, DIRECT_FETCH_BYTES)
    }

    /// `physical_start` supplies the proven contiguous instruction window. The stored
    /// completion policy consumes each selected path and its decoded operands.
    pub(crate) fn decode(
        &self,
        mut body: FunctionBuilder<'_>,
        instruction_eip: &Val<I32>,
        physical_start: Option<&Val<I32>>,
    ) -> Result<(), BuildError> {
        let mut cursor = RuntimeCursor::new(&body, self.fetch, instruction_eip, physical_start, 0)?;
        let opcode = cursor.byte(&mut body)?;
        self.opcode_handlers
            .tail_call(body, &cursor, DecodeState::default(), &[(&opcode).into()])
    }
}

impl DecodeState {
    fn return_unsupported(
        &self,
        body: FunctionBuilder<'_>,
        cursor: &RuntimeCursor<'_>,
        selector: &Val<I8>,
    ) -> Result<(), BuildError> {
        let opcode = match self.unsupported_opcode_override() {
            Some(opcode) => body.value::<I8>(u32::from(opcode))?,
            None => selector.clone(),
        };
        exit::unsupported(body, cursor.instruction_eip(), &opcode)
    }
}
