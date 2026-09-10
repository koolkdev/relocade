use wasm86_compiler::{BuildError, Func, FunctionBuilder, Program, Signature, Type, Val, I32};

use crate::{
    declare_dispatch, decode::RuntimeDecoder, execution::ExecutionBuilder,
    instruction::DecodedInstruction, memory::Memory, state::Cpu, CompiledModule,
};

/// Builds `step() -> i64`, which fetches and executes one supported instruction
/// at the current EIP, using the forms described in the
/// [crate documentation](crate). Addresses remain 32-bit for every data width.
/// Success publishes the instruction effects, EIP and instruction count,
/// then tail-calls `wasm86.dispatch(i32) -> i64` with the successor EIP. Relative
/// branches dispatch their target or fallthrough without fetching that instruction.
///
/// The module imports distinct `wasm86.cpuState`, `wasm86.guest` and
/// `wasm86.machine` memories, with minimum sizes of 1, 1 and 64 Wasm pages.
/// CPU fields match [`crate::compile_block_from_bytes`]. Machine memory contains
/// 2^20 little-endian 32-bit page-table entries starting at byte zero. Bit 0 marks
/// presence, bit 1 permits data writes, and bits 12 through 31 identify a 4-KiB
/// frame in guest memory. Reads require presence. Valid backing for present frames
/// is an internal invariant. Addresses are flat 32-bit sums:
/// segment bases are ignored, and effective-address arithmetic wraps at 32 bits.
///
/// A missing instruction page returns `(4 << 48) | (0x10 << 32) | address`, using
/// the first unavailable byte's 32-bit linear address. Data faults return
/// `(4 << 48) | (error << 32) | address`: error bit 1 identifies a write and bit 0
/// identifies a present but denied page. The address is the first denied byte.
/// This address-space policy rejects a data range crossing 0xffffffff
/// with a fault at its start (error 0 for a read, 2 for a write). A one-byte
/// access at 0xffffffff does not cross that boundary. Instruction fetch wraps.
/// All data permissions are checked before any guest store.
///
/// An unsupported instruction form returns `(8 << 48) | (opcode << 32) | EIP`, an
/// unsupported-subset exit rather than an architectural invalid-opcode exception.
/// `opcode` is the first byte after any `66` prefixes; EIP is the instruction start.
/// Group instructions reject an unsupported ModRM.reg extension before reading
/// their remaining fields. The diagnostic byte for an extended opcode is `0F`.
/// Supported forms fetch every
/// field before checking data access.
/// Faults and unsupported forms preserve this instruction's CPU state and
/// count and do not dispatch. EIP and count wrap at 32 bits; taken branches with `66`
/// truncate their targets to sixteen bits. Untaken branches keep full fallthrough EIP.
///
/// The `66` operand-size prefix selects word operands and leaves byte operands
/// unchanged. Repeating it does not toggle the width. Other prefixes are outside
/// the supported subset. All required instruction bytes count toward the 15-byte
/// limit. Attempting to read byte 16 returns general protection with error zero,
/// encoded as `2 << 48`. A missing required byte within the limit faults first.
/// This entry has no instruction budget or segment handling.
///
/// ```
/// let module = wasm86_x86::compile_interpreter_step()?;
/// assert_eq!(module.entry, "step");
/// # Ok::<(), wasm86_compiler::BuildError>(())
/// ```
pub fn compile_interpreter_step() -> Result<CompiledModule, BuildError> {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let memory = Memory::declare(&mut program)?;
    let dispatch = declare_dispatch(&mut program);
    let signature = Signature {
        parameters: vec![],
        result: Some(Type::I64),
    };
    let step = program.declare(signature.clone());
    let exact = program.declare(signature);
    let decoder = RuntimeDecoder::new(&mut program, &memory, |body, decoded| {
        complete(body, &cpu, &memory, dispatch, decoded)
    })?;

    let mut body = program.define(step)?;
    let start = cpu.read_eip(&mut body)?;
    let direct = decoder.direct_window(&mut body, &start)?;
    body.if_(&direct.unavailable, |arm| arm.tail_call(exact, &[]))?;
    decoder.decode(body, &start, Some(&direct.physical))?;

    let mut body = program.define(exact)?;
    let start = cpu.read_eip(&mut body)?;
    decoder.decode(body, &start, None)?;

    program.export("step", step)?;
    Ok(CompiledModule {
        bytes: program.compile()?,
        entry: "step".into(),
    })
}

fn complete(
    body: FunctionBuilder<'_>,
    cpu: &Cpu,
    memory: &Memory,
    dispatch: Func,
    decoded: DecodedInstruction<Val<I32>, Val<I32>>,
) -> Result<(), BuildError> {
    let mut execution = ExecutionBuilder::new(body, cpu, Some(memory), dispatch, &decoded.eip)?;
    execution.execute(decoded)?;
    execution.complete()
}
