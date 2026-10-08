use wasm86_compiler::{BuildError, Program, Signature, Type, I8};

use crate::{
    execution::ExecutionBuilder,
    memory::Memory,
    runtime::Runtime,
    state::{exit, Cpu},
    CompiledModule, ExecutionProfile,
};

/// Generates `deliver_interrupt(vector: i32) -> i64` for a host-selected Real16
/// maskable interrupt. The vector must be in 0..=255 and CPU state must be published.
/// IF clear or interrupt inhibition returns `0x0200_0000_0000_0000` unchanged.
/// Otherwise the interrupt is accepted: save current IP, CS and FLAGS, enter the
/// canonical IVT, and dispatch without retiring an instruction. Delivery faults
/// use the ordinary guest-fault exits. The host consumes an accepted vector even
/// when delivery faults; only the blocked result leaves it pending.
/// Host dispatch must not return the reserved blocked result after acceptance.
///
/// Call only at a published host boundary under compatible Real16 segment state. This
/// does not poll for interrupts inside blocks or between REP elements.
pub fn compile_real_mode_interrupt() -> Result<CompiledModule, BuildError> {
    let profile = ExecutionProfile::Real16;
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let memory = Memory::declare(&mut program, profile)?;
    let runtime = Runtime::declare(&mut program);
    let function = program.function(
        Signature {
            parameters: vec![Type::I8],
            results: vec![Type::I64],
        },
        |mut body| {
            let vector = body.parameter::<I8>(0)?;
            let enabled = cpu.accepts_maskable_interrupt(&mut body)?;
            body.if_(enabled.eq(false), exit::interrupt_blocked)?;
            let eip = cpu.read_eip(&mut body)?;
            let mut execution =
                ExecutionBuilder::new(body, &cpu, Some(&memory), runtime, &eip, profile)?;
            let target = execution.enter_real_mode_interrupt(vector, eip)?;
            execution.dispatch(target)
        },
    )?;
    program.export("deliver_interrupt", function)?;
    Ok(CompiledModule {
        bytes: program.compile()?,
        entry: "deliver_interrupt".into(),
        execution_profile: Some(profile),
    })
}
