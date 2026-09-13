use wasm86_compiler::{BuildError, Program, Signature, Type};

use crate::{
    declare_dispatch, decode::RuntimeDecoder, execution::ExecutionBuilder, memory::Memory,
    state::Cpu, CompiledModule, SegmentProfile,
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
/// is an internal invariant. The returned module requires [`SegmentProfile::Segmented32`]:
/// flat executable CS with 32-bit instruction defaults and SS.B=1. The host
/// establishes compatibility before entry. Data accesses check loaded segment
/// permissions and complete offset spans, then add the segment base at 32 bits.
///
/// A missing instruction page returns `(4 << 48) | (0x10 << 32) | address`, using
/// the first unavailable byte's 32-bit linear address. Data faults return
/// `(4 << 48) | (error << 32) | address`: error bit 1 identifies a write and bit 0
/// identifies a present but denied page. The address is the first denied byte.
/// Linear spans may wrap across zero and use the page mappings on both sides.
/// Segment violations return `16 << 48` for SS and `2 << 48` for other segments,
/// representing stack fault and general protection, both with error code zero.
/// All data permissions are checked before any guest store.
/// DIV/IDIV divide error returns `1 << 48`, with no error code or address payload.
/// It preserves the instruction's entry state and EIP without retiring or dispatching.
///
/// An unsupported instruction form returns `(8 << 48) | (opcode << 32) | EIP`, an
/// unsupported-subset exit rather than an architectural invalid-opcode exception.
/// `opcode` is the first byte after operand-size and segment prefixes; an unsupported form following
/// `F3` reports `F3`. EIP is the instruction start, including its prefixes.
/// Group instructions reject an unsupported ModRM.reg extension before reading
/// their remaining fields. The diagnostic byte for an extended opcode is `0F`.
/// Supported forms fetch every
/// field before checking data access.
/// Faults and unsupported forms do not retire or dispatch. Ordinary instruction
/// faults preserve entry CPU state. REP faults preserve successful elements and their
/// remaining ECX and current indices, without retiring the repeated instruction.
/// EIP and count wrap at 32 bits; taken branches with `66`
/// truncate their targets to sixteen bits. Untaken branches keep full fallthrough EIP.
///
/// The `66` operand-size prefix selects word operands and leaves byte operands
/// unchanged. Repeating it does not toggle the width. `F3` repeats MOVS/STOS using
/// full ECX, in either order with `66` and segment overrides. Repeated copies of
/// `66` or `F3` retain their effect; the last segment override wins. Zero ECX skips
/// data accesses; success retires the REP once and dispatches after the entire
/// instruction. `F2`, `67` and LOCK prefixes are outside
/// the supported subset. All required instruction bytes count toward the 15-byte
/// limit. Attempting to read byte 16 returns general protection with error zero,
/// encoded as `2 << 48`. A missing required byte within the limit faults first.
/// This entry has no instruction budget. Segment loads and 16-bit execution defaults
/// remain outside the subset.
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
    let profile = SegmentProfile::Segmented32;
    let signature = Signature {
        parameters: vec![],
        results: vec![Type::I64],
    };
    let step = program.declare(signature.clone());
    let exact = program.declare(signature);
    let decoder = RuntimeDecoder::new(&mut program, &memory, |body, decoded| {
        let mut execution =
            ExecutionBuilder::new(body, &cpu, Some(&memory), dispatch, &decoded.eip, profile)?;
        execution.execute(decoded)?;
        execution.complete()
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
        segment_profile: Some(profile),
    })
}
