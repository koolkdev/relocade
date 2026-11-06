//! Resources shared by successive DOS processes.

use crate::{
    devices::{BiosConsole, DeviceChain, NullDevice, Unavailable},
    files::FileSystem,
    process::{Process, Program},
    stdio::Stdio,
};
use relocade_machine::{ExitHandle, MemoryBus};
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

pub(crate) struct DosState {
    filesystem: RefCell<FileSystem>,
    stdio: Stdio,
    devices: DeviceChain,
    process: RefCell<Option<Rc<Process>>>,
    exit: ExitHandle,
}

impl DosState {
    pub(crate) fn new(
        memory: MemoryBus,
        keyboard: relocade_bios::Keyboard,
        exit: ExitHandle,
    ) -> Self {
        let devices = DeviceChain::default();
        devices.install(Unavailable(*b"PRN     "));
        devices.install(Unavailable(*b"AUX     "));
        devices.install(BiosConsole::new(memory, keyboard.clone()));
        devices.install(NullDevice);
        Self {
            filesystem: RefCell::default(),
            stdio: Stdio::new(keyboard),
            devices,
            process: RefCell::default(),
            exit,
        }
    }

    pub(crate) fn request_exit(&self) {
        self.exit.request();
    }

    pub(crate) fn filesystem(&self) -> RefMut<'_, FileSystem> {
        self.filesystem.borrow_mut()
    }

    pub(crate) fn devices(&self) -> &DeviceChain {
        &self.devices
    }

    pub(crate) fn stdio(&self) -> &Stdio {
        &self.stdio
    }

    pub(crate) fn process(&self) -> Option<Rc<Process>> {
        self.process.borrow().clone()
    }

    pub(crate) fn start_process(&self, program: Program) {
        *self.process.borrow_mut() = Some(Rc::new(Process::new(
            program,
            self.devices.clone(),
            &self.stdio,
        )));
    }
}
