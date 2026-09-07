use wasm86_compiler::{BuildError, Func, FunctionBuilder, Mem, Program, Signature, Type, Val, I32};

use crate::{
    declare_dispatch, decode,
    instruction::DecodedInstruction,
    memory::Memory,
    semantics,
    state::{self, State},
    CompiledModule,
};

/// Builds `step() -> i64`, which fetches and executes one unprefixed MOV32 at the
/// current EIP. Success updates the destination register, EIP and instruction count,
/// then tail-calls `wasm86.dispatch(i32) -> i64` with the next EIP.
///
/// The module imports distinct `wasm86.cpuState`, `wasm86.guest` and
/// `wasm86.machine` memories, with minimum sizes of 1, 1 and 64 Wasm pages.
/// CPU fields match [`crate::compile_block_from_bytes`]. Machine memory contains
/// 2^20 little-endian 32-bit page-table entries starting at byte zero. Bit 0 marks
/// presence; bits 12 through 31 identify a 4-KiB frame in guest memory. Present
/// frames must have valid backing. Invalid backing remains a Wasm trap.
///
/// A missing instruction page returns `(4 << 48) | (0x10 << 32) | address`, using
/// the first unavailable byte's 32-bit linear address. An opcode outside B8–BF
/// returns `(8 << 48) | (opcode << 32) | EIP`, an unsupported-subset exit rather
/// than an architectural invalid-opcode exception. Both preserve CPU state and
/// instruction count and do not dispatch. EIP and count wrap at 32 bits.
/// This entry has no instruction-budget or prefix handling.
///
/// ```
/// let module = wasm86_x86::compile_interpreter_step()?;
/// assert_eq!(module.entry, "step");
/// # Ok::<(), wasm86_compiler::BuildError>(())
/// ```
pub fn compile_interpreter_step() -> Result<CompiledModule, BuildError> {
    let mut program = Program::new();
    let cpu = state::declare(&mut program);
    let memory = Memory::declare(&mut program);
    let dispatch = declare_dispatch(&mut program);
    let signature = Signature {
        parameters: vec![],
        result: Type::I64,
    };
    let step = program.declare(signature.clone());
    let exact = program.declare(signature);

    let mut body = program.define(step)?;
    let start = state::read_eip(&mut body, cpu)?;
    let direct = decode::direct_window(&mut body, memory, &start)?;
    body.if_(&direct.unavailable, |arm| arm.tail_call(exact, &[]))?;
    let decoded = decode::live(&mut body, memory, &start, Some(&direct.physical))?;
    complete(body, cpu, dispatch, decoded)?;

    let mut body = program.define(exact)?;
    let start = state::read_eip(&mut body, cpu)?;
    let decoded = decode::live(&mut body, memory, &start, None)?;
    complete(body, cpu, dispatch, decoded)?;

    program.export("step", step)?;
    Ok(CompiledModule {
        bytes: program.compile()?,
        entry: "step".into(),
    })
}

fn complete(
    mut body: FunctionBuilder<'_>,
    cpu: Mem,
    dispatch: Func,
    decoded: DecodedInstruction<Val<I32>, Val<I32>>,
) -> Result<(), BuildError> {
    let mut state = State::new(cpu);
    semantics::lower(&mut body, &mut state, decoded.instruction)?;
    state.publish(&mut body, &decoded.next_eip, 1)?;
    body.tail_call(dispatch, &[decoded.next_eip.into()])
}
