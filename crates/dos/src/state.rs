//! Resources shared by successive DOS processes.

use crate::{files::FileSystem, process::Process, stdio::Stdio};
use relocade_machine::MemoryBus;
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

pub(crate) struct DosState {
    filesystem: RefCell<FileSystem>,
    stdio: Stdio,
    process: RefCell<Option<Rc<Process>>>,
    memory: MemoryBus,
}

impl DosState {
    pub(crate) fn new(memory: MemoryBus) -> Self {
        Self {
            filesystem: RefCell::default(),
            stdio: Stdio::default(),
            process: RefCell::default(),
            memory,
        }
    }

    pub(crate) fn memory(&self) -> &MemoryBus {
        &self.memory
    }

    pub(crate) fn filesystem(&self) -> RefMut<'_, FileSystem> {
        self.filesystem.borrow_mut()
    }

    pub(crate) fn stdio(&self) -> &Stdio {
        &self.stdio
    }

    pub(crate) fn process(&self) -> Option<Rc<Process>> {
        self.process.borrow().clone()
    }

    pub(crate) fn start_process(&self, psp_segment: u16) {
        *self.process.borrow_mut() = Some(Rc::new(Process::new(psp_segment)));
    }
}
