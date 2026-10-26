//! Resources shared by successive DOS processes.

use crate::{files::FileSystem, process::Process, stdio::Stdio};
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

#[derive(Default)]
pub(crate) struct DosState {
    filesystem: RefCell<FileSystem>,
    stdio: Stdio,
    process: RefCell<Option<Rc<Process>>>,
}

impl DosState {
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
