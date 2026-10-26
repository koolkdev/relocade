//! A launched DOS process and the resources released when it terminates.

mod startup;
pub(crate) use startup::{allocate, initial_cpu, COMMAND_TAIL_OFFSET, PSP_BYTES, PSP_PARAGRAPHS};

use crate::{
    files::{FileSystem, Files},
    memory, ServiceError,
};
use relocade_machine::MemoryView;
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

/// The lifecycle of one successfully launched DOS process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessStatus {
    /// Execution may continue, including after a budget stop or while awaiting input.
    Active,
    /// DOS terminated the process with an exit code or a service error.
    Exited(Result<u8, ServiceError>),
}

/// One DOS program's PSP identity, lifecycle and active resources.
pub(crate) struct Process {
    psp_segment: u16,
    state: RefCell<State>,
}

enum State {
    Active(Resources),
    Exited(Result<u8, ServiceError>),
}

struct Resources {
    files: Files,
    dta: DiskTransferAddress,
}

/// An active process borrowed for one DOS call.
pub(crate) struct ProcessContext<'call> {
    owner: &'call Rc<Process>,
    state: RefMut<'call, State>,
}

pub(crate) struct DiskTransferAddress {
    pub(crate) segment: u16,
    pub(crate) offset: u16,
}

impl DiskTransferAddress {
    pub(crate) fn address(&self) -> u32 {
        u32::from(self.segment) * 16 + u32::from(self.offset)
    }
}

impl Process {
    pub(crate) fn new(psp_segment: u16) -> Self {
        Self {
            psp_segment,
            state: RefCell::new(State::Active(Resources {
                files: Files::default(),
                dta: DiskTransferAddress {
                    segment: psp_segment,
                    offset: COMMAND_TAIL_OFFSET,
                },
            })),
        }
    }

    /// The PSP segment assigned at launch. Its memory may be reused after exit.
    pub(crate) fn psp_segment(&self) -> u16 {
        self.psp_segment
    }

    /// Reads this process's current status without retaining an execution borrow.
    pub(crate) fn status(&self) -> ProcessStatus {
        match &*self.state.borrow() {
            State::Active(_) => ProcessStatus::Active,
            State::Exited(status) => ProcessStatus::Exited(*status),
        }
    }

    pub(crate) fn context(self: &Rc<Self>) -> ProcessContext<'_> {
        ProcessContext {
            owner: self,
            state: self.state.borrow_mut(),
        }
    }
}

impl ProcessContext<'_> {
    pub(crate) fn psp_segment(&self) -> u16 {
        self.owner.psp_segment()
    }

    pub(crate) fn retain(&self) -> Rc<Process> {
        Rc::clone(self.owner)
    }

    fn resources(&mut self) -> &mut Resources {
        match &mut *self.state {
            State::Active(resources) => resources,
            State::Exited(_) => unreachable!("DOS service requires an active process"),
        }
    }

    pub(crate) fn files(&mut self) -> &mut Files {
        &mut self.resources().files
    }

    pub(crate) fn dta(&mut self) -> &mut DiskTransferAddress {
        &mut self.resources().dta
    }

    /// Records termination and releases resources through the existing state borrow.
    pub(crate) fn terminate(
        &mut self,
        status: Result<u8, ServiceError>,
        memory: &mut MemoryView<'_>,
        filesystem: &mut FileSystem,
    ) {
        let previous = std::mem::replace(&mut *self.state, State::Exited(status));
        let State::Active(resources) = previous else {
            unreachable!("DOS service requires an active process");
        };
        resources.files.close_all(filesystem);
        // Only the root process is supported; its exit releases the whole arena.
        memory::reset(memory);
    }
}
