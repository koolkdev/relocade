use wasm86_compiler::{BuildError, Program, Signature, Type};

use crate::{
    decode::{InstructionFetch, RuntimeDecoder},
    execution::ExecutionBuilder,
    memory::Memory,
    runtime::Runtime,
    state::Cpu,
    CompiledModule, SegmentProfile,
};

/// Builds `step() -> i64`, which fetches and executes one supported instruction
/// at the current EIP, using the forms described in the
/// [crate documentation](crate). The profile specializes CS.D instruction defaults.
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
/// is an internal invariant. The selected `profile` is a compilation assumption,
/// recorded in [`CompiledModule::segment_profile`]. Segmented16/Segmented32 assume
/// CS.D=0/1 respectively and handle SS.B at runtime. Flat32 also requires SS.B=1.
/// The host establishes
/// compatibility before entry and preserves it until a terminal segment load.
/// After committing a cache, the step only publishes state and dispatches; the
/// dispatch owner must admit the next entry against the new segment state.
/// [`SegmentProfile::Flat32`] omits segment checks and base reads for address
/// defaults and statically known DS/ES/SS accesses. It additionally requires
/// flat readable CS. Operands with an explicit segment override use complete checked
/// translation, while `66`, `67` and `F3` alone preserve the default-segment shortcut.
/// Both segmented profiles check all segments at runtime, including CS
/// for instruction fetch. EIP and dispatch targets are CS-relative offsets;
/// fetching adds CS.base before paging. Execute-only CS permits instruction fetch.
/// Checked accesses validate
/// permissions and complete offset spans, then add the segment base at 32 bits.
/// All variants have the same imports and entry signature, so
/// the host can instantiate them with shared memories and choose a compatible entry.
///
/// Segment loads call `wasm86.resolveSegment(segment: i32, selector: i32)`.
/// Segment indices are ES=0, CS=1, SS=2, DS=3, FS=4, GS=5; MOV never loads CS.
/// The host returns six i32 results: `(status, error_code, base, limit, selector,
/// attributes)`. Status zero returns a complete normalized [`crate::StoredSegment`];
/// otherwise status is architectural vector 11, 12 or 13 and only the error code
/// is used. Selector and attributes use zero-extended 16-bit values. Unknown
/// statuses or invalid successful records violate the host contract.
/// The callback resolves the current thread's descriptor view, for example through
/// [`crate::DescriptorTables::resolve_user_segment`]. It must not inspect or change
/// CPU state, guest RAM or page tables, or reenter guest execution. Wasm owns cache
/// commitment and fault publication. Resolver faults use the shared exception exit format:
/// `(tag << 48) | (error_code << 32)`, with tags 32 for #NP, 16 for #SS and 2 for #GP.
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
/// `opcode` is the first byte after size and segment prefixes; an unsupported form following
/// `F3` reports `F3`. EIP is the instruction start, including its prefixes.
/// Group instructions reject an unsupported ModRM.reg extension before reading
/// their remaining fields. The diagnostic byte for an extended opcode is `0F`.
/// Supported forms fetch every
/// field before checking data access.
/// Faults and unsupported forms do not retire or dispatch. Ordinary instruction
/// faults preserve entry CPU state. REP faults preserve successful elements and their
/// remaining count and current indices, without retiring the repeated instruction.
/// Taken near transfers validate their target against CS before changing ESP or
/// ECX. CALL checks the target before pushing; RET reads the stack before checking
/// its target and committing ESP. Target paging belongs to the next fetch.
/// An untaken branch does not check its unused target or its fallthrough offset.
/// EIP and retired-instruction count wrap at 32 bits. Operand size truncates taken
/// targets; address size independently selects CX/ECX. Untaken branches keep full fallthrough EIP.
///
/// CS.D sets operand/address defaults. `66` and `67` independently select the
/// opposite width; byte operands stay byte-sized. `F3` repeats MOVS/STOS using
/// address-sized CX/ECX and SI/DI or ESI/EDI. Prefix order is unrestricted and
/// repetitions preserve presence; the last segment override wins. Zero count skips
/// data accesses; success retires the REP once and dispatches after the entire
/// instruction. `F2` and LOCK prefixes are outside
/// the supported subset. All required instruction bytes count toward the 15-byte
/// limit. A direct fetch window must satisfy both CS and paging. If it does not,
/// required bytes are checked in order, with CS checked before paging for each byte.
/// Attempting to read byte 16 returns general protection with error zero,
/// encoded as `2 << 48`. A missing required byte within the limit faults first.
/// This entry has no instruction budget. MOV reads visible selectors and loads
/// ES/SS/DS/FS/GS through the resolver. Segment PUSH/POP and far transfers remain
/// outside the subset. Interrupt/debug delivery and the inhibition following
/// MOV SS are not modeled.
///
/// ```
/// use wasm86_x86::{compile_interpreter_step, SegmentProfile};
/// let module = compile_interpreter_step(SegmentProfile::Flat32)?;
/// assert_eq!(module.entry, "step");
/// # Ok::<(), wasm86_compiler::BuildError>(())
/// ```
pub fn compile_interpreter_step(profile: SegmentProfile) -> Result<CompiledModule, BuildError> {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let memory = Memory::declare(&mut program)?;
    let runtime = Runtime::declare(&mut program);
    let signature = Signature {
        parameters: vec![],
        results: vec![Type::I64],
    };
    let step = program.declare(signature.clone());
    let exact = program.declare(signature);
    let fetch = InstructionFetch::new(&cpu, &memory, profile);
    let decoder = RuntimeDecoder::new(
        &mut program,
        fetch,
        profile.code_default_size(),
        |body, decoded| {
            let mut execution =
                ExecutionBuilder::new(body, &cpu, Some(&memory), runtime, &decoded.eip, profile)?;
            execution.execute(decoded)?;
            execution.complete()
        },
    )?;

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
