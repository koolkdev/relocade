//! Resources shared by successive DOS processes.

use crate::{files::FileSystem, process::Process, stdio::Stdio};
use relocade_machine::{MemoryBus, StopHandle};
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

pub(crate) struct DosState {
    filesystem: RefCell<FileSystem>,
    stdio: Stdio,
    process: RefCell<Option<Rc<Process>>>,
    memory: MemoryBus,
    stop: StopHandle,
}

impl DosState {
    pub(crate) fn new(
        memory: MemoryBus,
        keyboard: relocade_bios::Keyboard,
        stop: StopHandle,
    ) -> Self {
        Self {
            filesystem: RefCell::default(),
            stdio: Stdio::new(keyboard),
            process: RefCell::default(),
            memory,
            stop,
        }
    }

    pub(crate) fn memory(&self) -> &MemoryBus {
        &self.memory
    }

    pub(crate) fn stop(&self) {
        self.stop.stop();
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
