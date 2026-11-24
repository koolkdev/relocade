//! Construction of the interpreter program shared by its run and step entries.

use relocade_compiler::{BuildError, Program};

use crate::{
    decode::RuntimeDecoder, execution::ExecutionModule, state::State, CompilerConfig,
    EntryAssumptions, ExecutionProfile, ExecutionTarget, MemoryBackend,
};

/// Builds one interpreter program exporting both `run` and `step`.
/// Compile the returned program with [`Program::compile`] or [`Program::compile_with`].
/// Entry selection happens when invoking the resulting module.
/// CPU features and instruction budgets are read from the live execution context;
/// the program depends only on the execution profile and does not specialize CPU state.
///
/// `run() -> ExecutionExit` fetches and executes instructions until a branch, segment load,
/// POPF/POPFD, port I/O or serializing instruction completes, then
/// tail-calls host dispatch.
/// Real16 STI from IF=0 and MOV/POP SS instead return `InterruptInhibited`;
/// the embedding must execute their protected successor through `step`.
/// Conditional branches dispatch on both outcomes without fetching the successor.
/// Supported Real16 faults enter their IVT handler and return `InterruptEntered`.
/// Other faults and unsupported forms return directly with earlier work published.
///
/// Each instruction reads live guest bytes and starts with fresh prefix state.
/// The host must establish profile compatibility before entry, as described by
/// the [host integration contract](crate#host-integration).
///
/// Execution interrupts and instruction budgets are checked only before dispatch.
/// Straight-line execution continues until a block-ending instruction or guest exit.
/// REP completes its repetition; port strings dispatch and other strings continue.
/// A fault retains completed elements.
/// The snapshot compiler's instruction limit does not bound interpreter execution.
///
/// `step() -> ExecutionExit` fetches and executes one supported instruction at the current
/// CS-relative EIP under the selected execution profile.
/// Success publishes state, retires the instruction and returns `StepComplete`
/// without fetching or dispatching the successor.
/// An inhibiting Real16 STI/MOV SS/POP SS instead returns `InterruptInhibited`,
/// requesting another protected step before IRQ polling or an execution interrupt.
/// Real16 POPF/IRET that enables IF returns `InterruptsEnabled`.
/// HLT returns `Halted` with IP after the instruction; the host must retain the halt.
/// Supported Real16 faults enter their IVT handler and return `InterruptEntered`
/// without executing the handler or retiring the faulting instruction.
/// Other faults and unsupported forms return directly without retiring it.
/// REP executes all elements before returning and retains completed elements on a fault.
/// Completed instructions check execution interrupts and instruction budgets before
/// returning, except `InterruptInhibited` and `Halted`, whose driver transitions
/// must happen first.
///
/// The host must establish profile compatibility before entry. The interpreter
/// checks instruction fetches at runtime; it does not require a validated snapshot.
/// See the [host integration contract](crate#host-integration) for imports,
/// dispatch, entry validity and fault encoding.
///
/// ```
/// use relocade_x86::{build_interpreter, SegmentProfile};
/// let program = build_interpreter(SegmentProfile::Flat32)?;
/// let bytes = program.compile()?;
/// // The same module contains both entry exports.
/// # Ok::<(), relocade_compiler::BuildError>(())
/// ```
pub fn build_interpreter(profile: impl Into<ExecutionProfile>) -> Result<Program, BuildError> {
    let profile = profile.into();
    build_interpreter_with_memory(profile, MemoryBackend::default_for(profile))
}

/// Builds the physical-bus PC interpreter. Both `step` and `run` read execution mode,
/// CS.D, SS.B and the loaded segment caches from the entering CPU. The host establishes
/// ordinary real mode or the supported protected user state, with paging and VM86 disabled.
/// Real-mode interrupts require the conventional IVT. A protected instruction that needs
/// a guest descriptor-table lookup currently returns [`crate::ExecutionExit::UnsupportedInstruction`].
pub fn build_pc_interpreter() -> Result<Program, BuildError> {
    build_interpreter_with_config(EntryAssumptions::LIVE, MemoryBackend::PhysicalBus)
}

/// Builds the same `run` and `step` entries as [`build_interpreter`] with explicit
/// memory routing. Both instruction fetch and data accesses use the selected backend
/// after segment checks. Protected host-page entries require IF set and IOPL zero.
/// For a protected-mode PC with guest paging disabled and live IF and IOPL, choose
/// [`MemoryBackend::PhysicalBus`].
pub fn build_interpreter_with_memory(
    profile: impl Into<ExecutionProfile>,
    memory_backend: MemoryBackend,
) -> Result<Program, BuildError> {
    let config = CompilerConfig::from(ExecutionTarget::new(profile.into(), memory_backend));
    build_interpreter_with_config(config.entry, memory_backend)
}

/// Builds the `run` and `step` interpreter entries for the supplied CPU guarantees
/// and memory backend. CPU features and instruction budgets come from the executing context.
pub fn build_interpreter_with_config(
    entry: EntryAssumptions,
    memory_backend: MemoryBackend,
) -> Result<Program, BuildError> {
    let mut program = Program::new();
    let execution =
        ExecutionModule::declare(&mut program, CompilerConfig::new(entry, memory_backend));
    execution.export_entries(&mut program)?;
    let before_cpu = execution.cpu.clone();
    let decoder = RuntimeDecoder::new(
        &mut program,
        execution.clone(),
        move |body| {
            let single_step = RuntimeDecoder::single_step(body)?;
            body.if_(single_step, |mut body| {
                let state = State::new(&before_cpu);
                state.checkpoint(&mut body)?;
                body.return_(crate::ExecutionExitValues::step_complete())
            })
        },
        move |body, decoded, continuation| {
            let single_step = RuntimeDecoder::single_step(&body)?;
            let ends_block = decoded.instruction.ends_block();
            let mut execution = execution
                .builder(body, &decoded.eip)?
                .with_cpu_features_from_context()?
                .with_interpreter_controls(single_step);
            execution.execute(decoded)?;
            if ends_block {
                execution.dispatch()
            } else {
                execution.complete(|body, _| continuation.resume(body))
            }
        },
    )?;

    for (export, single_step) in [(crate::abi::RUN, false), (crate::abi::STEP, true)] {
        let wrapper = program.declare(export.signature());
        let decoder = decoder.clone();
        program.define(wrapper, move |body| {
            let single_step = body.value::<relocade_compiler::I1>(single_step)?;
            decoder.enter(body, &single_step)
        })?;
        export.export(&mut program, wrapper)?;
    }
    Ok(program)
}

#[cfg(test)]
mod tests;
