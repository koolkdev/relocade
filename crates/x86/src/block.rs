use wasm86_compiler::{Program, Signature, Type};

use crate::{
    decode, execution::ExecutionBuilder, memory::Memory, runtime::Runtime, state::Cpu, BlockError,
    CompiledModule, SegmentProfile,
};

/// Compiles a byte snapshot under [`SegmentProfile::Flat32`].
///
/// Compilation stops at the first branch, segment load, unconditional fault or
/// `instruction_limit`. A conditional branch ends the block on both outcomes;
/// bytes after the boundary are ignored. The limit must be nonzero. Incomplete,
/// unsupported or overlong instructions within that boundary return [`BlockError`].
///
/// The generated `block_<hex start_eip>() -> i64` entry executes the block and
/// tail-calls host dispatch on success; guest faults return directly.
/// The host must satisfy the profile and snapshot-validity requirements documented
/// by [`compile_block_from_bytes_with_profile`].
///
/// ```
/// use wasm86_x86::compile_block_from_bytes;
/// let module = compile_block_from_bytes(0x1000, &[0xb8, 42, 0, 0, 0], 1)?;
/// assert_eq!(module.entry, "block_1000");
/// # Ok::<(), wasm86_x86::BlockError>(())
/// ```
pub fn compile_block_from_bytes(
    start_eip: u32,
    bytes: &[u8],
    instruction_limit: u32,
) -> Result<CompiledModule, BlockError> {
    compile_block_from_bytes_with_profile(
        start_eip,
        bytes,
        instruction_limit,
        SegmentProfile::Flat32,
    )
}

/// Compiles a byte snapshot under the selected segment profile.
/// The stopping boundary and exported entry follow [`compile_block_from_bytes`].
/// The profile determines instruction defaults; size prefixes independently
/// select the opposite operand or address width.
///
/// Before each entry, the host must establish profile compatibility and validate
/// the full fetch spans and bytes of every compiled instruction against CS and
/// guest memory. Generated blocks do not repeat instruction-fetch checks. Changes
/// to relevant CS state, code bytes or mappings require revalidation or invalidation
/// of affected entries and direct links. See the
/// [host integration contract](crate#entry-validity) for validity throughout
/// execution and fallback at an invalid fetch.
///
/// ```
/// use wasm86_x86::{compile_block_from_bytes_with_profile, SegmentProfile};
///
/// // MOV AX, 0x1234 under 16-bit code defaults.
/// let block = compile_block_from_bytes_with_profile(
///     0x1000, &[0xb8, 0x34, 0x12], 1, SegmentProfile::Segmented16,
/// )?;
/// assert_eq!(block.segment_profile, Some(SegmentProfile::Segmented16));
/// # Ok::<(), wasm86_x86::BlockError>(())
/// ```
pub fn compile_block_from_bytes_with_profile(
    start_eip: u32,
    bytes: &[u8],
    instruction_limit: u32,
    profile: SegmentProfile,
) -> Result<CompiledModule, BlockError> {
    if instruction_limit == 0 {
        return Err(BlockError::ZeroInstructionLimit);
    }

    let mut decoded_instructions = Vec::new();
    let mut remaining_bytes = bytes;
    let mut next_eip = start_eip;
    for _ in 0..instruction_limit {
        let (decoded_instruction, rest) =
            decode::snapshot(remaining_bytes, next_eip, profile.code_default_size())?;
        next_eip = decoded_instruction.fallthrough_eip;
        remaining_bytes = rest;
        let ends_block = decoded_instruction.instruction.ends_block();
        decoded_instructions.push(decoded_instruction);
        if ends_block {
            break;
        }
    }

    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let memory = decoded_instructions
        .iter()
        .any(|decoded_instruction| decoded_instruction.instruction.uses_memory())
        .then(|| Memory::declare(&mut program))
        .transpose()?;
    let runtime = Runtime::declare(&mut program);
    let function = program.function(
        Signature {
            parameters: vec![],
            results: vec![Type::I64],
        },
        |body| {
            let mut execution =
                ExecutionBuilder::new(body, &cpu, memory.as_ref(), runtime, start_eip, profile)?;
            for decoded_instruction in decoded_instructions {
                execution.execute(decoded_instruction)?;
            }
            execution.complete(|body, eip| runtime.dispatch(body, eip))
        },
    )?;
    let entry = format!("block_{start_eip:x}");
    program.export(&entry, function)?;
    Ok(CompiledModule {
        bytes: program.compile()?,
        entry,
        segment_profile: Some(profile),
    })
}
