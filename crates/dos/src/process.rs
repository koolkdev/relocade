//! A launched DOS process and the resources released when it terminates.

mod startup;
pub(crate) use startup::{allocate, initial_cpu, COMMAND_TAIL_OFFSET, PSP_BYTES, PSP_PARAGRAPHS};

use crate::{
    files::{FileSystem, Files},
    memory, ServiceError,
};
use relocade_machine::MemoryView;
use std::cell::{Ref, RefCell, RefMut};

/// The lifecycle of one successfully launched DOS process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessStatus {
    /// Execution may continue, including after a budget stop or while awaiting input.
    Active,
    /// DOS terminated the process with an exit code or a service error.
    Exited(Result<u8, ServiceError>),
}

/// One DOS program's PSP identity, lifecycle and active resources.
///
/// Obtain an owned reference through [`crate::Dos::process`]. It remains valid
/// across execution and later launches. Termination releases files, searches and
/// conventional memory even when the host retains this process reference.
pub struct Process {
    psp_segment: u16,
    lifecycle: RefCell<Lifecycle>,
}

enum Lifecycle {
    Active(Resources),
    Exited(Result<u8, ServiceError>),
}

pub(crate) struct Resources {
    pub(crate) files: Files,
    pub(crate) dta_segment: u16,
    pub(crate) dta_offset: u16,
}

impl Process {
    pub(crate) fn new(psp_segment: u16) -> Self {
        Self {
            psp_segment,
            lifecycle: RefCell::new(Lifecycle::Active(Resources {
                files: Files::default(),
                dta_segment: psp_segment,
                dta_offset: COMMAND_TAIL_OFFSET,
            })),
        }
    }

    /// The PSP segment assigned at launch. Its memory may be reused after exit.
    pub fn psp_segment(&self) -> u16 {
        self.psp_segment
    }

    /// Reads this process's current status without retaining an execution borrow.
    pub fn status(&self) -> ProcessStatus {
        match &*self.lifecycle.borrow() {
            Lifecycle::Active(_) => ProcessStatus::Active,
            Lifecycle::Exited(status) => ProcessStatus::Exited(*status),
        }
    }

    pub(crate) fn resources(&self) -> Ref<'_, Resources> {
        Ref::map(self.lifecycle.borrow(), |lifecycle| match lifecycle {
            Lifecycle::Active(resources) => resources,
            Lifecycle::Exited(_) => unreachable!("DOS service requires an active process"),
        })
    }

    pub(crate) fn resources_mut(&self) -> RefMut<'_, Resources> {
        RefMut::map(self.lifecycle.borrow_mut(), |lifecycle| match lifecycle {
            Lifecycle::Active(resources) => resources,
            Lifecycle::Exited(_) => unreachable!("DOS service requires an active process"),
        })
    }

    pub(crate) fn finish(
        &self,
        memory: &mut MemoryView<'_>,
        filesystem: &mut FileSystem,
        status: Result<u8, ServiceError>,
    ) {
        let previous = self.lifecycle.replace(Lifecycle::Exited(status));
        if let Lifecycle::Active(resources) = previous {
            resources.files.close_all(filesystem);
            // Only the root process is supported; its exit releases the whole arena.
            memory::reset(memory);
        }
    }
}
