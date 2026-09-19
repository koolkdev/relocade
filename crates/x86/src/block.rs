use wasm86_compiler::{Program, Signature, Type};

use crate::{
    decode, execution::ExecutionBuilder, memory::Memory, runtime::Runtime, state::Cpu, BlockError,
    CompiledModule, SegmentProfile,
};

/// Compiles from `start_eip` through the first branch, REP, segment load or `instruction_limit`
/// instructions, whichever comes first. A conditional branch ends the block
/// on both outcomes. Bytes after that boundary are ignored.
/// Supports the instruction forms described in the
/// [crate documentation](crate). Operand and address sizes default to 32 bits;
/// `66` and `67` independently select 16 bits. `F3` repeats MOVS/STOS.
/// Incomplete, unsupported or overlong instructions are construction errors. This byte-only
/// input carries no guest-fault information.
/// EIP and the completed-instruction count use 32-bit wrapping arithmetic;
/// a taken branch with `66` additionally truncates its target to sixteen bits.
///
/// The exported `block_<hex start_eip>` function has signature `() -> i64` and
/// imports `wasm86.cpuState`, a memory of at least one 64-KiB page. Its little-endian
/// 32-bit fields are EAX, ECX, EDX, EBX, ESP, EBP, ESI and EDI at offsets 24 through
/// 52 in steps of four, EIP at 56 and the completed-instruction count at 144.
/// Byte and word register writes preserve the remaining bits of their parent register.
/// Overlapping views synchronize through CPU backing when required; final dirty
/// definitions are written in first-write order, followed by EIP and count. The block
/// then tail-calls the imported `wasm86.dispatch(i32) -> i64` with the successor EIP
/// and returns its result.
/// A branch selects its target or fallthrough without fetching another instruction.
///
/// Blocks with data-memory operands also import guest RAM and the page table,
/// using the layout and fault words documented by [`crate::compile_interpreter_step`].
/// The returned module requires [`SegmentProfile::Flat32`]. The host establishes
/// compatibility before entry and keeps it valid until a terminal segment load.
/// Dispatch reestablishes compatibility before entering subsequent code.
/// DS/ES/SS accesses use those flat assumptions without runtime segment
/// guards, as do CS reads; FS/GS accesses and CS writes check their loaded caches.
/// A data fault publishes earlier completed instructions, keeps EIP at the faulting
/// instruction, and skips dispatch.
/// REP also preserves successful elements and their address-sized count and index
/// progress. It retires once after all elements succeed, including a zero count.
/// DIV/IDIV divide error returns `1 << 48` with that same completion boundary.
/// Blocks that load a segment also import `wasm86.resolveSegment`, using the
/// resolver contract documented by [`crate::compile_interpreter_step`]. A load
/// commits its cache only on success, retires once, and ends the block.
/// All bytes of a store are permission-checked before any of them are written.
/// Read-modify-write operations check write permission before reading their
/// destination or changing flags; CMP and TEST require only read permission.
/// Status flags use the lazy CPU record
/// described in the [crate documentation](crate).
/// The host must maintain the snapshot validity described by
/// [`compile_block_from_bytes_with_profile`], which also accepts segmented profiles.
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
/// The entry signature, stopping boundary and state publication follow
/// [`compile_block_from_bytes`]. The profile specializes CS.D during decoding;
/// operand and address overrides independently select the opposite width.
/// Segmented profiles handle data segments and SS.B at runtime.
/// Taken near targets are checked separately, without accessing their pages.
///
/// The caller must have validated instruction fetches for every instruction
/// compiled into the block: CS must permit the complete instruction spans, and
/// the bytes must match readable guest memory at CS.base + start_eip, with 32-bit
/// linear wrapping. Bytes beyond the compilation boundary are ignored.
/// EIP and dispatch targets are CS-relative offsets.
///
/// The host must establish snapshot validity and profile compatibility before
/// every entry, including direct dispatch links, and preserve both until a
/// terminal instruction changes them. After a segment cache commit, only state
/// publication and dispatch remain; the next entry must be admitted afresh.
/// Changes to relevant CS state, code bytes or mappings require
/// revalidation or invalidation of affected entries and links. An instruction
/// that changes relied-upon assumptions must end the block before further
/// execution under them. Profile compatibility alone does not prove fetch validity.
///
/// This byte-only compiler cannot validate CS limits, permissions or code pages;
/// the generated block does not recheck them for instruction fetches. A checked
/// snapshot producer must stop before an invalid fetch and execute any valid
/// instruction prefix before handling that guest fault, for example by entering
/// the interpreter at the failing instruction. A debug assertion in the block
/// owner can detect a violated validity invariant; it does not replace guest faults.
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
            execution.complete()
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
