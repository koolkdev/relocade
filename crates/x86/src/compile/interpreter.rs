use relocade_compiler::{abi::Function, BuildError, Program, I64};

use crate::{
    decode::{InstructionFetch, RuntimeDecoder},
    execution::ExecutionBuilder,
    memory::Memory,
    runtime::Runtime,
    state::{Cpu, State},
    CompiledModule, ExecutionProfile,
};

use super::Compiler;

/// Generates a module exporting both `step` and `run`, selecting `run() -> i64`.
/// This entry fetches and executes instructions until a branch, segment load,
/// port I/O or serializing instruction completes, then
/// tail-calls host dispatch.
/// Conditional branches dispatch on both outcomes without fetching the successor.
/// Guest faults and unsupported forms return directly with earlier work published.
///
/// Each instruction reads live guest bytes and starts with fresh prefix state.
/// The host must establish profile compatibility before entry, as described by
/// the [host integration contract](crate#host-integration).
///
/// Instruction budgets are checked only before dispatch. Straight-line execution
/// continues until a block-ending instruction or guest exit. REP completes its repetition; port
/// strings dispatch and other strings continue. A fault retains completed elements.
/// The snapshot compiler's instruction limit does not bound interpreter execution.
///
/// ```
/// use relocade_x86::{compile_interpreter, SegmentProfile};
/// let module = compile_interpreter(SegmentProfile::Flat32)?;
/// assert_eq!(module.entry, "run");
/// # Ok::<(), relocade_compiler::BuildError>(())
/// ```
pub fn compile_interpreter(
    profile: impl Into<ExecutionProfile>,
) -> Result<CompiledModule, BuildError> {
    Compiler::new(profile).compile_interpreter()
}

/// Generates the same module as [`compile_interpreter`], selecting `step() -> i64`.
/// This entry fetches and executes one supported instruction at the current
/// CS-relative EIP under the selected execution profile.
/// Success publishes state, retires the instruction and tail-calls host dispatch.
/// Guest faults and unsupported forms return directly without retiring it.
/// REP executes all elements before dispatch and retains completed elements on a fault.
/// This entry checks the instruction target after retiring the instruction.
///
/// The host must establish profile compatibility before entry. The interpreter
/// checks instruction fetches at runtime; it does not require a validated snapshot.
/// See the [host integration contract](crate#host-integration) for imports,
/// dispatch, entry validity and fault encoding.
///
/// ```
/// use relocade_x86::{compile_interpreter_step, SegmentProfile};
/// let module = compile_interpreter_step(SegmentProfile::Flat32)?;
/// assert_eq!(module.entry, "step");
/// # Ok::<(), relocade_compiler::BuildError>(())
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
    fn symbol(self) -> Function<'static, fn() -> I64> {
        match self {
            Self::Run => crate::abi::RUN,
            Self::Step => crate::abi::STEP,
        }
    }
}

impl Compiler {
    /// Generates the runtime-decoded block entry described by [`compile_interpreter`]
    /// with this compiler's execution target. CPU observations are not used.
    /// Feature choices are read from `ExecutionContext::cpu_model`; the host must
    /// initialize that field to the target's model before entry.
    pub fn compile_interpreter(&self) -> Result<CompiledModule, BuildError> {
        self.compile_runtime(InterpreterEntry::Run)
    }

    /// Generates the single-instruction entry described by [`compile_interpreter_step`]
    /// with this compiler's execution target. CPU observations are not used.
    /// Feature choices are read from `ExecutionContext::cpu_model`; the host must
    /// initialize that field to the target's model before entry.
    pub fn compile_interpreter_step(&self) -> Result<CompiledModule, BuildError> {
        self.compile_runtime(InterpreterEntry::Step)
    }

    fn compile_runtime(&self, entry: InterpreterEntry) -> Result<CompiledModule, BuildError> {
        let profile = self.target.profile;
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let memory = Memory::declare(&mut program, profile)?;
        let runtime = Runtime::declare(&mut program);
        let fetch = InstructionFetch::new(&cpu, &memory, profile);
        let decoder = RuntimeDecoder::new(
            &mut program,
            fetch,
            profile.code_default_size(),
            |body| {
                let single_step = RuntimeDecoder::single_step(body)?;
                body.if_(single_step, |mut body| {
                    let state = State::new(&cpu);
                    let count = state.instruction_count(&mut body, 0)?;
                    state.check_instruction_budget(&mut body, &count)?;
                    runtime.dispatch(body)
                })
            },
            |body, decoded, continuation| {
                let ends_block = decoded.instruction.ends_block();
                let mut execution = ExecutionBuilder::new(
                    body,
                    &cpu,
                    Some(&memory),
                    runtime,
                    &decoded.eip,
                    self.target,
                )?
                .with_cpu_features_from_context()?;
                execution.execute(decoded)?;
                if ends_block {
                    execution.dispatch()
                } else {
                    execution.complete(|body, _| continuation.resume(body))
                }
            },
        )?;

        for export in [InterpreterEntry::Run, InterpreterEntry::Step] {
            let wrapper = program.declare(export.symbol().signature());
            program.define(wrapper, |body| {
                let single_step =
                    body.value::<relocade_compiler::I1>(matches!(export, InterpreterEntry::Step))?;
                decoder.enter(body, &single_step)
            })?;
            export.symbol().export(&mut program, wrapper)?;
        }
        Ok(CompiledModule {
            bytes: program.compile()?,
            entry: entry.symbol().name.into(),
            execution_target: Some(self.target),
        })
    }
}
