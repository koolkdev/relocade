mod address;
mod cursor;
mod cycle;
mod fetch;
mod handlers;
mod operands;
mod selectors;

use wasm86_compiler::{BuildError, Func, FunctionBuilder, Label, Program, Val, I1, I32, I8};

use crate::{
    instruction::{DecodedInstruction, OpcodeMap, PrefixState},
    memory::DirectRange,
    segment::SegmentDefaultSize,
    state::exit,
};

use super::DecodeState;

pub(crate) use fetch::InstructionFetch;

use self::{
    cursor::{RuntimeCursor, DIRECT_FETCH_BYTES},
    handlers::{DecodeHandlers, DecodePoint},
};

/// The decoder owns the destination after successful instruction publication.
/// A scoped continuation stays within the current opcode function; other entries
/// restart through the interpreter entry with fresh decoding state.
pub(crate) enum DecodeContinuation {
    Entry(Func),
    Cycle(Label<()>),
}

impl DecodeContinuation {
    pub(crate) fn resume(&self, body: FunctionBuilder<'_>) -> Result<(), BuildError> {
        match self {
            Self::Entry(function) => body.tail_call(*function, &[]),
            Self::Cycle(label) => body.branch(label, ()),
        }
    }
}

/// Declares runtime decoding entries and builds their instruction paths.
pub(crate) struct RuntimeDecoder<'memory> {
    fetch: InstructionFetch<'memory>,
    default_size: SegmentDefaultSize,
    opcode_handlers: DecodeHandlers,
    primary_modrm_memory_handlers: DecodeHandlers,
    extended_modrm_memory_handlers: DecodeHandlers,
}

/// Decodes instructions in one function scope with its completion policy.
struct InstructionDecoder<'decoder, 'memory, C> {
    decoder: &'decoder RuntimeDecoder<'memory>,
    complete_instruction: C,
    // Carries the extended-map selector, form index and ModRM to memory decoding.
    memory_continuation: Option<Label<(I1, I32, I8)>>,
}

impl<'memory> RuntimeDecoder<'memory> {
    pub(crate) fn new(
        program: &mut Program,
        fetch: InstructionFetch<'memory>,
        default_size: SegmentDefaultSize,
        loop_entry: Option<Func>,
        complete_instruction: impl Fn(
            FunctionBuilder<'_>,
            DecodedInstruction<Val<I32>, Val<I32>>,
            Option<&DecodeContinuation>,
        ) -> Result<(), BuildError>,
    ) -> Result<Self, BuildError> {
        let decoder = Self {
            fetch,
            default_size,
            opcode_handlers: DecodeHandlers::declare(
                program,
                DecodePoint::Opcode,
                default_size,
                true,
            ),
            primary_modrm_memory_handlers: DecodeHandlers::declare(
                program,
                DecodePoint::MemoryOperand(OpcodeMap::Primary),
                default_size,
                loop_entry.is_none(),
            ),
            extended_modrm_memory_handlers: DecodeHandlers::declare(
                program,
                DecodePoint::MemoryOperand(OpcodeMap::Extended),
                default_size,
                loop_entry.is_none(),
            ),
        };
        let continuation = loop_entry.map(DecodeContinuation::Entry);
        for handlers in [
            &decoder.primary_modrm_memory_handlers,
            &decoder.extended_modrm_memory_handlers,
        ] {
            handlers.define(program, fetch, |body, cursor, state| {
                let form_index = body.parameter::<I32>(1)?;
                let modrm = body.parameter::<I8>(2)?;
                InstructionDecoder {
                    decoder: &decoder,
                    memory_continuation: None,
                    complete_instruction: |body: FunctionBuilder<'_>, instruction| {
                        complete_instruction(body, instruction, continuation.as_ref())
                    },
                }
                .decode_memory_operands(body, cursor, state, &form_index, &modrm)
            })?;
        }
        decoder
            .opcode_handlers
            .define(program, fetch, |body, cursor, state| {
                let opcode = body.parameter::<I8>(1)?;
                if let Some(entry) = loop_entry.filter(|_| cursor.physical_start().is_some()) {
                    decoder.decode_cycle(body, cursor, &opcode, entry, &complete_instruction)
                } else {
                    InstructionDecoder {
                        decoder: &decoder,
                        memory_continuation: None,
                        complete_instruction: |body: FunctionBuilder<'_>, instruction| {
                            complete_instruction(body, instruction, continuation.as_ref())
                        },
                    }
                    .decode_opcode(body, cursor, state, &opcode)
                }
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
            .check_direct_access(body, instruction_eip, DIRECT_FETCH_BYTES, None)
    }

    /// `physical_start` supplies the proven contiguous instruction window.
    pub(crate) fn decode(
        &self,
        mut body: FunctionBuilder<'_>,
        instruction_eip: &Val<I32>,
        physical_start: Option<&Val<I32>>,
    ) -> Result<(), BuildError> {
        let mut cursor = RuntimeCursor::new(&body, self.fetch, instruction_eip, physical_start, 0)?;
        let opcode = cursor.byte(&mut body)?;
        self.opcode_handlers.tail_call(
            body,
            &cursor,
            DecodeState {
                prefixes: PrefixState::new(self.default_size),
                ..DecodeState::default()
            },
            &[(&opcode).into()],
        )
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
