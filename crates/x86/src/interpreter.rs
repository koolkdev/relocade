use wasm86_compiler::{BuildError, Program, Signature, Type};

use crate::{
    decode::{InstructionFetch, RuntimeDecoder},
    execution::ExecutionBuilder,
    memory::Memory,
    runtime::Runtime,
    state::Cpu,
    CompiledModule, SegmentProfile,
};

/// Generates `step() -> i64`, which fetches and executes one supported instruction
/// at the current CS-relative EIP under the selected segment profile.
/// Success publishes state, retires the instruction and tail-calls host dispatch.
/// Guest faults and unsupported forms return directly without retiring it.
/// REP executes all elements before dispatch and retains completed elements on a fault.
/// This entry has no execution budget.
///
/// The host must establish profile compatibility before entry. The interpreter
/// checks instruction fetches at runtime; it does not require a validated snapshot.
/// See the [host integration contract](crate#host-integration) for imports,
/// dispatch, entry validity and fault encoding.
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
