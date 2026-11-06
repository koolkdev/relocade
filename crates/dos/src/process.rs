//! A launched DOS process and the resources released when it terminates.

mod startup;
mod vectors;
pub(crate) use startup::{allocate, initial_cpu, COMMAND_TAIL_OFFSET, PSP_BYTES, PSP_PARAGRAPHS};

use crate::{
    devices::DeviceChain,
    files::{FileSystem, Files},
    memory,
    stdio::Stdio,
    ServiceError,
};
use relocade_machine::{FarPointer, MemoryView};
use std::cell::{RefCell, RefMut};

/// The lifecycle of one successfully launched DOS process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessStatus {
    /// DOS has not reported termination, including while awaiting input.
    Active,
    /// DOS terminated the process with an exit code or a service error.
    Exited(Result<u8, ServiceError>),
}

/// One DOS program's PSP identity and lifecycle.
/// Retaining this process preserves its status after another program starts.
/// Termination releases its active resources regardless of retained references.
/// Dropping the DOS session does not report a process exit; retained handles can
/// keep the last active state and its resources alive.
pub struct Process {
    psp_segment: u16,
    state: RefCell<State>,
}

enum State {
    Active(Box<Resources>),
    Exited(Result<u8, ServiceError>),
}

struct Resources {
    files: Files,
    dta: FarPointer,
}

impl Process {
    pub(crate) fn new(psp_segment: u16, devices: DeviceChain, stdio: &Stdio) -> Self {
        Self {
            psp_segment,
            state: RefCell::new(State::Active(Box::new(Resources {
                files: Files::new(devices, stdio),
                dta: FarPointer {
                    segment: psp_segment,
                    offset: COMMAND_TAIL_OFFSET,
                },
            }))),
        }
    }

    /// The PSP segment assigned at launch. Its memory may be reused after exit.
    pub fn psp_segment(&self) -> u16 {
        self.psp_segment
    }

    /// Reads this process's current status without retaining an execution borrow.
    pub fn status(&self) -> ProcessStatus {
        match &*self.state.borrow() {
            State::Active(_) => ProcessStatus::Active,
            State::Exited(status) => ProcessStatus::Exited(*status),
        }
    }

    fn resources(&self) -> RefMut<'_, Resources> {
        RefMut::map(self.state.borrow_mut(), |state| match state {
            State::Active(resources) => resources.as_mut(),
            State::Exited(_) => unreachable!("DOS service requires an active process"),
        })
    }

    pub(crate) fn files(&self) -> RefMut<'_, Files> {
        RefMut::map(self.resources(), |resources| &mut resources.files)
    }

    pub(crate) fn dta(&self) -> RefMut<'_, FarPointer> {
        RefMut::map(self.resources(), |resources| &mut resources.dta)
    }

    /// Records termination and releases the active resources immediately.
    pub(crate) fn terminate(
        &self,
        status: Result<u8, ServiceError>,
        memory: &mut MemoryView<'_>,
        filesystem: &mut FileSystem,
    ) {
        let previous = self.state.replace(State::Exited(status));
        let State::Active(resources) = previous else {
            unreachable!("DOS service requires an active process");
        };
        vectors::restore(memory, self.psp_segment);
        resources.files.close_all(filesystem);
        // Only the root process is supported; its exit releases the whole arena.
        // Its caller is the host, so the restored INT 22h is not executed.
        memory::reset(memory);
    }
}
