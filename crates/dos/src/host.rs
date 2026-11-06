//! Guest storage for a DOS process implemented in Rust.

mod interrupt;
mod registers;
pub use interrupt::{InterruptCall, InterruptError, InterruptOutcome, InterruptResult};
pub use registers::InterruptRegisters;

use crate::{memory, process, LoadError, LoadOptions};
use relocade_machine::{Cpu, CpuContext, FarPointer};
use std::cell::{Cell, RefCell};

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
    installed: Cell<bool>,
    invocation: RefCell<Option<Invocation>>,
}

struct Invocation {
    completed_exits: u64,
    buffer_len: usize,
    returned: Option<InterruptRegisters>,
}

impl HostProgram {
    fn caller(&self) -> FarPointer {
        FarPointer {
            segment: self.segment,
            offset: CALLER_OFFSET,
        }
    }

    fn buffer(&self) -> FarPointer {
        FarPointer {
            segment: self.segment,
            offset: BUFFER_OFFSET,
        }
    }

    fn completion(&self) -> u32 {
        self.caller().address() + u32::from(CALLER_BYTES - 1)
    }

    pub(crate) fn remove_hook(&self, cpu: &mut Cpu) {
        if self.installed.replace(false) {
            cpu.remove_entries([self.completion()]);
        }
    }
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
    Ok(process::Program::Host(HostProgram {
        segment,
        installed: Cell::new(false),
        invocation: RefCell::new(None),
    }))
}
