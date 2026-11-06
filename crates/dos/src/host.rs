//! Guest storage for a DOS process implemented in Rust.

use crate::{memory, process, LoadError, LoadOptions};
use relocade_machine::CpuContext;

// The caller is INT imm8 followed by its return position. Transfer storage and
// the downward-growing IRQ stack follow it in the process's own allocation.
const CALLER_BYTES: u16 = 3;
const CALLER_OFFSET: u16 = process::PSP_BYTES as u16;
const BUFFER_OFFSET: u16 = CALLER_OFFSET + CALLER_BYTES;
const BUFFER_BYTES: u16 = 1024;
const STACK_BYTES: u16 = 4096;
const STACK_TOP: u16 = (BUFFER_OFFSET + BUFFER_BYTES + STACK_BYTES).next_multiple_of(2);
const PARAGRAPHS: usize = (STACK_TOP as usize).div_ceil(memory::PARAGRAPH_BYTES as usize);

pub(crate) struct HostProgram {
    pub(crate) segment: u16,
}

pub(crate) fn load(
    context: &mut CpuContext<'_>,
    options: &LoadOptions<'_>,
) -> Result<process::Program, LoadError> {
    let allocation = process::allocate(&mut context.memory, PARAGRAPHS, PARAGRAPHS, options)?;
    let segment = allocation.segment;
    // Until Rust supplies a call, the guest entry has an ordinary DOS exit.
    context
        .memory
        .write(
            u32::from(segment) * memory::PARAGRAPH_BYTES + u32::from(CALLER_OFFSET),
            &[0xcd, 0x20, 0x90], // INT 20h; NOP.
        )
        .expect("host process caller");
    let mut cpu = process::initial_cpu(segment);
    cpu.registers.esp = u32::from(STACK_TOP);
    cpu.eip = u32::from(CALLER_OFFSET);
    context
        .cpu
        .restore(&cpu)
        .expect("host process entry satisfies the real-mode execution target");
    Ok(process::Program::Host(HostProgram { segment }))
}
