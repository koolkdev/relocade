//! Call a guest interrupt and retain its invocation across dropped Rust waiters.

use super::{InterruptRegisters, Invocation, BUFFER_BYTES, STACK_TOP};
use crate::{Dos, ProcessStatus, ServiceError};
use relocade_machine::{Clock, FarPointer, Hook, Runner};
use relocade_x86::{ExecutionExit, HostExit, Segment, SegmentRegister};
use std::{fmt, rc::Rc};

/// One guest interrupt call. `buffer` is copied to [`Dos::interrupt_buffer`]
/// after admission; use that pointer in the register arguments when needed.
/// At most 1024 bytes fit. The returned buffer has the same length as the input.
pub struct InterruptCall<'a> {
    pub vector: u8,
    pub registers: InterruptRegisters,
    pub buffer: &'a [u8],
}

/// State observed at the caller's post-INT continuation.
#[derive(Debug)]
pub struct InterruptResult {
    pub registers: InterruptRegisters,
    pub buffer: Vec<u8>,
}

/// A normal interrupt return is distinct from DOS terminating its caller.
#[derive(Debug)]
pub enum InterruptOutcome {
    Returned(InterruptResult),
    Exited(Result<u8, ServiceError>),
}

#[derive(Debug)]
pub enum InterruptError {
    NoHostProcess,
    Busy,
    NoPendingCall,
    BufferTooLarge,
    Setup(relocade_machine::Error),
    /// An external execution exit abandoned the call without returning from INT.
    Aborted,
    /// Guest execution stopped; the invocation remains available to resume.
    Execution(ExecutionExit),
}

impl fmt::Display for InterruptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHostProcess => f.write_str("no Rust DOS process is available"),
            Self::Busy => f.write_str("the CPU has an unfinished interrupt call"),
            Self::NoPendingCall => f.write_str("no interrupt call is awaiting completion"),
            Self::BufferTooLarge => f.write_str("interrupt transfer buffer exceeds 1024 bytes"),
            Self::Setup(error) => error.fmt(f),
            Self::Aborted => f.write_str("execution exited without returning from the interrupt"),
            Self::Execution(exit) => write!(f, "interrupt execution stopped: {exit:?}"),
        }
    }
}

impl std::error::Error for InterruptError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Setup(error) => Some(error),
            _ => None,
        }
    }
}

impl<C: Clock> Dos<C> {
    /// The active Rust process's transfer buffer. Calls copy their input here
    /// only after checking that no earlier invocation owns it.
    pub fn interrupt_buffer(&self) -> Result<FarPointer, InterruptError> {
        let process = self.state.process().ok_or(InterruptError::NoHostProcess)?;
        if process.status() != ProcessStatus::Active {
            return Err(InterruptError::NoHostProcess);
        }
        process
            .host_program()
            .map(|program| program.buffer())
            .ok_or(InterruptError::NoHostProcess)
    }

    /// Executes an actual INT instruction through the current guest vector table.
    /// The process's allocated caller, buffer and stack remain owned while the
    /// CPU awaits DOS/BIOS services or executes guest interrupt handlers.
    ///
    /// Dropping this waiter preserves an entered invocation, including its buffer.
    /// Recover it with [`Self::resume_interrupt`]; a new call returns Busy without
    /// changing guest state. Cancellation during hook setup has not entered a call.
    pub async fn call_interrupt(
        &mut self,
        runner: &Runner<C>,
        call: InterruptCall<'_>,
    ) -> Result<InterruptOutcome, InterruptError> {
        let process = self.state.process().ok_or(InterruptError::NoHostProcess)?;
        let program = process
            .host_program()
            .ok_or(InterruptError::NoHostProcess)?;
        if process.status() != ProcessStatus::Active {
            return Err(InterruptError::NoHostProcess);
        }
        if program.invocation.borrow().is_some() || self.cpu.has_pending_host_calls() {
            return Err(InterruptError::Busy);
        }
        if call.buffer.len() > usize::from(BUFFER_BYTES) {
            return Err(InterruptError::BufferTooLarge);
        }
        if !program.installed.get() {
            let owner = Rc::downgrade(&process);
            let state = self.cpu.cpu_state();
            let exit = self.cpu.exit_handle();
            self.cpu
                .install([Hook::host(program.completion(), async move || {
                    let process = owner.upgrade().expect("active host process");
                    let program = process.host_program().expect("host process caller");
                    program
                        .invocation
                        .borrow_mut()
                        .as_mut()
                        .expect("entered interrupt call")
                        .returned = Some(InterruptRegisters::read(&state));
                    exit.request();
                })])
                .await
                .map_err(InterruptError::Setup)?;
            program.installed.set(true);
        }
        {
            let memory = self.cpu.memory();
            let mut view = memory.view();
            view.write(program.buffer().address(), call.buffer)
                .expect("host transfer buffer");
            view.write(program.caller().address(), &[0xcd, call.vector, 0x90])
                .expect("host INT caller");
        }
        let state = self.cpu.cpu_state();
        call.registers.write(&state);
        state.set_segment(
            Segment::Cs,
            SegmentRegister::real_mode(Segment::Cs, program.segment),
        );
        state.set_segment(
            Segment::Ss,
            SegmentRegister::real_mode(Segment::Ss, program.segment),
        );
        state.set_sp(STACK_TOP);
        state.set_eip(u32::from(program.caller().offset));
        *program.invocation.borrow_mut() = Some(Invocation {
            completed_exits: self.cpu.exit_handle().completed_exits(),
            buffer_len: call.buffer.len(),
            returned: None,
        });
        self.resume_interrupt(runner).await
    }

    /// Continues the exact invocation left by a dropped waiter or guest stop.
    /// A completed but unclaimed call returns its retained result without running
    /// the CPU again. An external HostExit::Return abandons the invocation.
    pub async fn resume_interrupt(
        &mut self,
        runner: &Runner<C>,
    ) -> Result<InterruptOutcome, InterruptError> {
        let process = self.state.process().ok_or(InterruptError::NoHostProcess)?;
        let program = process
            .host_program()
            .ok_or(InterruptError::NoHostProcess)?;
        let returned = program
            .invocation
            .borrow()
            .as_ref()
            .ok_or(InterruptError::NoPendingCall)?
            .returned
            .is_some();
        if !returned && process.status() == ProcessStatus::Active {
            let completed_exits = program
                .invocation
                .borrow()
                .as_ref()
                .unwrap()
                .completed_exits;
            if self.cpu.exit_handle().completed_exits() != completed_exits {
                program.invocation.borrow_mut().take();
                return Err(InterruptError::Aborted);
            }
            let exit = runner.run_cpu(&mut self.cpu).await;
            if exit != ExecutionExit::Host(HostExit::Return) {
                return Err(InterruptError::Execution(exit));
            }
        }
        let invocation = program
            .invocation
            .borrow_mut()
            .take()
            .expect("retained interrupt call");
        if let ProcessStatus::Exited(status) = process.status() {
            program.remove_hook(&mut self.cpu);
            return Ok(InterruptOutcome::Exited(status));
        }
        let registers = invocation.returned.ok_or(InterruptError::Aborted)?;
        let mut buffer = vec![0; invocation.buffer_len];
        self.cpu
            .memory()
            .view()
            .read(program.buffer().address(), &mut buffer)
            .expect("host transfer buffer");
        Ok(InterruptOutcome::Returned(InterruptResult {
            registers,
            buffer,
        }))
    }
}
