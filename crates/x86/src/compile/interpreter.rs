use wasm86_compiler::{BuildError, Program, Signature, Type};

use crate::{
    decode::{InstructionFetch, RuntimeDecoder},
    execution::ExecutionBuilder,
    memory::Memory,
    runtime::Runtime,
    state::Cpu,
    CompiledModule, ExecutionProfile,
};

use super::Compiler;

/// Generates `run() -> i64`, which fetches and executes instructions until a
/// branch, segment load or port I/O completes, then tail-calls host dispatch.
/// Conditional branches dispatch on both outcomes without fetching the successor.
/// Guest faults and unsupported forms return directly with earlier work published.
///
/// Each instruction reads live guest bytes and starts with fresh prefix state.
/// The host must establish profile compatibility before entry, as described by
/// the [host integration contract](crate#host-integration).
///
/// This entry has no execution budget. Straight-line execution continues until
/// a block-ending instruction or guest exit. REP completes its repetition and
/// continues to the next instruction; a fault retains completed elements.
/// The snapshot compiler's instruction limit does not bound interpreter execution.
///
/// ```
/// use wasm86_x86::{compile_interpreter, SegmentProfile};
/// let module = compile_interpreter(SegmentProfile::Flat32)?;
/// assert_eq!(module.entry, "run");
/// # Ok::<(), wasm86_compiler::BuildError>(())
/// ```
pub fn compile_interpreter(
    profile: impl Into<ExecutionProfile>,
) -> Result<CompiledModule, BuildError> {
    Compiler::new(profile).compile_interpreter()
}

/// Generates `step() -> i64`, which fetches and executes one supported instruction
/// at the current CS-relative EIP under the selected execution profile.
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
pub fn compile_interpreter_step(
    profile: impl Into<ExecutionProfile>,
) -> Result<CompiledModule, BuildError> {
    Compiler::new(profile).compile_interpreter_step()
}

#[derive(Clone, Copy)]
enum InterpreterEntry {
    Run,
    Step,
}

impl InterpreterEntry {
    fn name(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Step => "step",
        }
    }
}

impl Compiler {
    /// Generates the runtime-decoded block entry described by [`compile_interpreter`]
    /// with this compiler's execution profile. CPU observations are not used.
    pub fn compile_interpreter(&self) -> Result<CompiledModule, BuildError> {
        self.compile_runtime(InterpreterEntry::Run)
    }

    /// Generates the single-instruction entry described by [`compile_interpreter_step`]
    /// with this compiler's execution profile. CPU observations are not used.
    pub fn compile_interpreter_step(&self) -> Result<CompiledModule, BuildError> {
        self.compile_runtime(InterpreterEntry::Step)
    }

    fn compile_runtime(&self, entry: InterpreterEntry) -> Result<CompiledModule, BuildError> {
        let profile = self.profile;
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let memory = Memory::declare(&mut program, profile)?;
        let runtime = Runtime::declare(&mut program);
        let signature = Signature {
            parameters: vec![],
            results: vec![Type::I64],
        };
        let entry_function = program.declare(signature.clone());
        let exact = program.declare(signature);
        let fetch = InstructionFetch::new(&cpu, &memory, profile);
        let decoder = RuntimeDecoder::new(
            &mut program,
            fetch,
            profile.code_default_size(),
            matches!(entry, InterpreterEntry::Run).then_some(entry_function),
            |body, decoded, continuation| {
                let continuation =
                    continuation.filter(|_| !decoded.instruction.ends_block(profile));
                let mut execution = ExecutionBuilder::new(
                    body,
                    &cpu,
                    Some(&memory),
                    runtime,
                    &decoded.eip,
                    profile,
                )?;
                execution.execute(decoded)?;
                execution.complete(|body, eip| match continuation {
                    Some(continuation) => continuation.resume(body),
                    None => runtime.dispatch(body, eip),
                })
            },
        )?;

        program.define(entry_function, |mut body| {
            let start = cpu.read_eip(&mut body)?;
            let direct = decoder.direct_window(&mut body, &start)?;
            body.if_(&direct.unavailable, |arm| arm.tail_call(exact, &[]))?;
            decoder.decode(body, &start, Some(&direct.physical))
        })?;

        program.define(exact, |mut body| {
            let start = cpu.read_eip(&mut body)?;
            decoder.decode(body, &start, None)
        })?;

        program.export(entry.name(), entry_function)?;
        Ok(CompiledModule {
            bytes: program.compile()?,
            entry: entry.name().into(),
            execution_profile: Some(profile),
        })
    }
}
