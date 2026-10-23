mod address;
mod cursor;
mod cycle;
mod fetch;
mod handlers;
mod operands;
mod selectors;

use relocade_compiler::{
    BlockBuilder, BuildError, Func, Label, Program, Signature, Type, Val, I1, I32, I8,
};

use crate::{
    instruction::{DecodedInstruction, OpcodeMap, PrefixState},
    segment::SegmentDefaultSize,
    state::exit,
};

use super::DecodeState;

pub(crate) use fetch::InstructionFetch;

use self::{
    cursor::RuntimeCursor,
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
    pub(crate) fn resume(&self, body: BlockBuilder<'_>) -> Result<(), BuildError> {
        match self {
            Self::Entry(function) => RuntimeDecoder::resume(body, *function),
            Self::Cycle(label) => body.branch(label, ()),
        }
    }
}

/// Declares runtime decoding entries and builds their instruction paths.
pub(crate) struct RuntimeDecoder<'memory> {
    fetch: InstructionFetch<'memory>,
    entry: Func,
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
        before_next_instruction: impl Fn(&mut BlockBuilder<'_>) -> Result<(), BuildError>,
        complete_instruction: impl Fn(
            BlockBuilder<'_>,
            DecodedInstruction<Val<I32>, Val<I32>>,
            &DecodeContinuation,
        ) -> Result<(), BuildError>,
    ) -> Result<Self, BuildError> {
        let signature = Signature {
            parameters: vec![Type::I1],
            results: crate::abi::RUN.signature().results,
        };
        let entry = program.declare(signature.clone());
        let exact = program.declare(signature.clone());
        let completed = program.declare(signature);
        let decoder = Self {
            fetch,
            entry,
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
                false,
            ),
            extended_modrm_memory_handlers: DecodeHandlers::declare(
                program,
                DecodePoint::MemoryOperand(OpcodeMap::Extended),
                default_size,
                false,
            ),
        };
        let continuation = DecodeContinuation::Entry(completed);
        for handlers in [
            &decoder.primary_modrm_memory_handlers,
            &decoder.extended_modrm_memory_handlers,
        ] {
            handlers.define(program, &decoder.fetch, |body, cursor, state| {
                let form_index = body.parameter::<I32>(2)?;
                let modrm = body.parameter::<I8>(3)?;
                InstructionDecoder {
                    decoder: &decoder,
                    memory_continuation: None,
                    complete_instruction: |body: BlockBuilder<'_>, instruction| {
                        complete_instruction(body, instruction, &continuation)
                    },
                }
                .decode_memory_operands(body, cursor, state, &form_index, &modrm)
            })?;
        }
        decoder
            .opcode_handlers
            .define(program, &decoder.fetch, |body, cursor, state| {
                let opcode = body.parameter::<I8>(2)?;
                if cursor.physical_start().is_some() {
                    decoder.decode_cycle(
                        body,
                        cursor,
                        &opcode,
                        entry,
                        &before_next_instruction,
                        &complete_instruction,
                    )
                } else {
                    InstructionDecoder {
                        decoder: &decoder,
                        memory_continuation: None,
                        complete_instruction: |body: BlockBuilder<'_>, instruction| {
                            complete_instruction(body, instruction, &continuation)
                        },
                    }
                    .decode_opcode(body, cursor, state, &opcode)
                }
            })?;
        for (function, after_instruction) in [(entry, false), (completed, true)] {
            program.define(function, |mut body| {
                if after_instruction {
                    before_next_instruction(&mut body)?;
                }
                decoder.start(body, exact)
            })?;
        }
        program.define(exact, |mut body| {
            let fetch = decoder.fetch.clone();
            let start = fetch.eip(&mut body)?;
            decoder.decode(body, fetch, &start, None)
        })?;
        Ok(decoder)
    }

    /// Every private decoder entry carries the execution policy in parameter zero.
    /// Keeping it in the activation makes nested host calls independent.
    pub(crate) fn single_step(body: &BlockBuilder<'_>) -> Result<Val<I1>, BuildError> {
        body.parameter::<I1>(0)
    }

    fn resume(body: BlockBuilder<'_>, entry: Func) -> Result<(), BuildError> {
        let single_step = Self::single_step(&body)?;
        body.tail_call(entry, &[(&single_step).into()])
    }

    /// Public exports choose a policy; decoder continuations retain it privately.
    pub(crate) fn enter(
        &self,
        body: BlockBuilder<'_>,
        single_step: &Val<I1>,
    ) -> Result<(), BuildError> {
        body.tail_call(self.entry, &[single_step.into()])
    }

    /// Both initial entry and helper completion fetch directly, avoiding a second
    /// tail call on prefixed instructions. Exact fallback retains the policy.
    fn start(&self, mut body: BlockBuilder<'_>, exact: Func) -> Result<(), BuildError> {
        let mut fetch = self.fetch.clone();
        let start = fetch.eip(&mut body)?;
        let direct = fetch.direct_window(&mut body, &start)?;
        body.if_(&direct.unavailable, |body| Self::resume(body, exact))?;
        self.decode(body, fetch, &start, Some(&direct.physical))
    }

    /// `physical_start` supplies the proven contiguous instruction window.
    fn decode(
        &self,
        mut body: BlockBuilder<'_>,
        fetch: InstructionFetch<'memory>,
        instruction_eip: &Val<I32>,
        physical_start: Option<&Val<I32>>,
    ) -> Result<(), BuildError> {
        let mut cursor = RuntimeCursor::new(&body, fetch, instruction_eip, physical_start, 0)?;
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
        body: BlockBuilder<'_>,
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
