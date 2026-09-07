use wasm86_compiler::{Program, Signature, Type};

use crate::{declare_dispatch, decode, semantics, state, BlockError, CompiledModule};

/// Compiles exactly `instruction_limit` instructions starting at `start_eip`.
/// Supports unprefixed MOV imm32 to a register and register-to-register MOV
/// (`89`/`8B` with ModRM.mod = 3). Bytes after the selection are ignored.
/// Missing or unsupported selected bytes are construction errors. This byte-only
/// input carries no guest-fault information.
/// EIP and the completed-instruction count advance with 32-bit wrapping arithmetic.
///
/// The exported `block_<hex start_eip>` function has signature `() -> i64` and
/// imports `wasm86.cpuState`, a memory of at least one 64-KiB page. Its little-endian
/// 32-bit fields are EAX, ECX, EDX, EBX, ESP, EBP, ESI and EDI at offsets 24 through
/// 52 in steps of four, EIP at 56 and the completed-instruction count at 144.
/// Other bytes are preserved. Final register values are written in first-write
/// order, followed by EIP and count. The block then tail-calls the imported
/// `wasm86.dispatch(i32) -> i64` with the next EIP and returns its result.
pub fn compile_block_from_bytes(
    start_eip: u32,
    bytes: &[u8],
    instruction_limit: u32,
) -> Result<CompiledModule, BlockError> {
    if instruction_limit == 0 {
        return Err(BlockError::ZeroInstructionLimit);
    }

    let mut program = Program::new();
    let memory = state::declare(&mut program);
    let dispatch = declare_dispatch(&mut program);
    let function = program.declare(Signature {
        parameters: vec![],
        result: Type::I64,
    });
    let mut body = program.define(function)?;
    let mut state = state::State::new(memory);
    let mut remaining = bytes;
    let mut next_eip = start_eip;

    for _ in 0..instruction_limit {
        let (decoded, rest) = decode::snapshot(remaining, next_eip)?;
        semantics::lower(&mut body, &mut state, decoded.instruction)?;
        remaining = rest;
        next_eip = decoded.next_eip;
    }

    state.publish(&mut body, next_eip, instruction_limit)?;
    body.tail_call(dispatch, &[next_eip.into()])?;
    let entry = format!("block_{start_eip:x}");
    program.export(&entry, function)?;
    Ok(CompiledModule {
        bytes: program.compile()?,
        entry,
    })
}
