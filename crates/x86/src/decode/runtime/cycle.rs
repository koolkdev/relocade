//! Shared continuation for the ordinary direct opcode path.

use wasm86_compiler::{BuildError, Func, FunctionBuilder, Val, I1, I32, I8};

use crate::{
    instruction::{DecodedInstruction, OpcodeMap, PrefixState},
    memory::{PageCache, PageCacheInputs},
};

use super::{
    cursor::{RuntimeCursor, DIRECT_FETCH_BYTES},
    DecodeContinuation, DecodeState, InstructionDecoder, RuntimeDecoder,
};

impl RuntimeDecoder<'_> {
    pub(super) fn decode_cycle(
        &self,
        mut body: FunctionBuilder<'_>,
        cursor: RuntimeCursor<'_>,
        opcode: &Val<I8>,
        entry: Func,
        complete_instruction: &impl Fn(
            FunctionBuilder<'_>,
            DecodedInstruction<Val<I32>, Val<I32>>,
            Option<&DecodeContinuation>,
        ) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        let initial = (
            cursor.instruction_eip(),
            opcode,
            cursor.physical_start().expect("direct opcode entry"),
            PageCache::EMPTY,
        );
        body.loop_::<(I32, I8, I32, PageCacheInputs), ()>(
            initial,
            |mut iteration, labels, (start, opcode, physical, cache)| {
                let mut cache = PageCache::from_inputs(cache);
                iteration.block::<()>(|body, next| {
                    let cursor = RuntimeCursor::new(&body, self.fetch, &start, Some(&physical), 1)?;
                    let continuation = DecodeContinuation::Cycle(next);
                    InstructionDecoder {
                        decoder: self,
                        complete_instruction: |body: FunctionBuilder<'_>, instruction| {
                            complete_instruction(body, instruction, Some(&continuation))
                        },
                        memory_continuation: None,
                    }
                    .decode_direct(body, cursor, &opcode)
                })?;
                // Each completed instruction publishes before this shared fetch.
                // A helper transfer leaves the activation and discards its cache.
                let start = self.fetch.eip(&mut iteration)?;
                let direct = self.fetch.check_direct_access(
                    &mut iteration,
                    &start,
                    DIRECT_FETCH_BYTES,
                    Some(&mut cache),
                )?;
                iteration.if_(&direct.unavailable, |fallback| {
                    fallback.tail_call(entry, &[])
                })?;
                let mut cursor =
                    RuntimeCursor::new(&iteration, self.fetch, &start, Some(&direct.physical), 0)?;
                let opcode = cursor.byte(&mut iteration)?;
                iteration.branch(
                    &labels.again,
                    (start, opcode, direct.physical, cache.into_inputs()),
                )
            },
        )?;
        body.trap()
    }
}

impl<C> InstructionDecoder<'_, '_, C>
where
    C: Fn(FunctionBuilder<'_>, DecodedInstruction<Val<I32>, Val<I32>>) -> Result<(), BuildError>,
{
    /// Memory forms share their address decoder within the same iteration.
    /// Register forms complete through the enclosing instruction continuation.
    fn decode_direct(
        &self,
        mut body: FunctionBuilder<'_>,
        cursor: RuntimeCursor<'_>,
        opcode: &Val<I8>,
    ) -> Result<(), BuildError> {
        let prefixes = PrefixState::new(self.decoder.default_size);
        let (extended, form_index, modrm) = body.block::<(I1, I32, I8)>(|body, memory| {
            InstructionDecoder {
                decoder: self.decoder,
                complete_instruction: &self.complete_instruction,
                memory_continuation: Some(memory),
            }
            .decode_opcode(
                body,
                cursor.clone(),
                DecodeState {
                    prefixes: prefixes.clone(),
                    ..DecodeState::default()
                },
                opcode,
            )
        })?;
        let decode_memory = |body: FunctionBuilder<'_>, map| {
            let cursor = RuntimeCursor::new(
                &body,
                self.decoder.fetch,
                cursor.instruction_eip(),
                cursor.physical_start(),
                super::handlers::DecodePoint::MemoryOperand(map).consumed(),
            )?;
            self.decode_memory_operands(
                body,
                cursor,
                DecodeState {
                    prefixes: prefixes.clone(),
                    map,
                },
                &form_index,
                &modrm,
            )
        };
        body.if_else(
            extended,
            |body| decode_memory(body, OpcodeMap::Extended),
            |body| decode_memory(body, OpcodeMap::Primary),
        )?;
        body.trap()
    }
}
