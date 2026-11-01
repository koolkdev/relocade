//! Resources shared by successive DOS processes.

use crate::{files::FileSystem, process::Process, stdio::Stdio};
use relocade_machine::{ExitHandle, MemoryBus};
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

pub(crate) struct DosState {
    filesystem: RefCell<FileSystem>,
    stdio: Stdio,
    process: RefCell<Option<Rc<Process>>>,
    memory: MemoryBus,
    exit: ExitHandle,
}

impl DosState {
    pub(crate) fn new(
        memory: MemoryBus,
        keyboard: relocade_bios::Keyboard,
        exit: ExitHandle,
    ) -> Self {
        Self {
            filesystem: RefCell::default(),
            stdio: Stdio::new(keyboard),
            process: RefCell::default(),
            memory,
            exit,
        }
    }

    pub(crate) fn memory(&self) -> &MemoryBus {
        &self.memory
    }

    pub(crate) fn request_exit(&self) {
        self.exit.request();
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
