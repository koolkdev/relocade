use wasm86_compiler::{Program, Signature, Type};

use crate::{
    declare_dispatch, decode, execution::ExecutionBuilder, memory::Memory, state::Cpu, BlockError,
    CompiledModule,
};

/// Compiles from `start_eip` through the first branch, REP or `instruction_limit`
/// instructions, whichever comes first. A conditional branch ends the block
/// on both outcomes. Bytes after that boundary are ignored.
/// Supports the instruction forms described in the
/// [crate documentation](crate). ModRM/SIB addressing and absolute offsets are
/// 32-bit. The `66` operand-size prefix selects word operands; `F3` repeats MOVS/STOS.
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
/// Addresses are flat: segment bases are ignored. A data fault publishes earlier
/// completed instructions, keeps EIP at the faulting instruction, and skips dispatch.
/// REP also preserves successful elements and their ECX/ESI/EDI progress. It retires
/// once after all elements succeed, including when ECX starts at zero.
/// DIV/IDIV divide error returns `1 << 48` with that same completion boundary.
/// All bytes of a store are permission-checked before any of them are written.
/// Read-modify-write operations check write permission before reading their
/// destination or changing flags; CMP and TEST require only read permission.
/// Status flags use the lazy CPU record
/// described in the [crate documentation](crate).
pub fn compile_block_from_bytes(
    start_eip: u32,
    bytes: &[u8],
    instruction_limit: u32,
) -> Result<CompiledModule, BlockError> {
    if instruction_limit == 0 {
        return Err(BlockError::ZeroInstructionLimit);
    }

    let mut decoded_instructions = Vec::new();
    let mut remaining_bytes = bytes;
    let mut next_eip = start_eip;
    for _ in 0..instruction_limit {
        let (decoded_instruction, rest) = decode::snapshot(remaining_bytes, next_eip)?;
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
    let dispatch = declare_dispatch(&mut program);
    let function = program.function(
        Signature {
            parameters: vec![],
            results: vec![Type::I64],
        },
        |body| {
            let mut execution =
                ExecutionBuilder::new(body, &cpu, memory.as_ref(), dispatch, start_eip)?;
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
    })
}
