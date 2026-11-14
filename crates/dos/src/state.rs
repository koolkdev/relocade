//! Resources shared by successive DOS processes.

mod clock;
pub(crate) mod indos;

use crate::{
    devices::{BiosConsole, ConsoleInput, DeviceChain, NullDevice, Unavailable},
    files::FileSystem,
    process::Process,
    stdio::Stdio,
};
use relocade_machine::{ExitHandle, MemoryBus};
use std::{
    cell::{Cell, RefCell, RefMut},
    rc::Rc,
};

pub(crate) struct DosState {
    pub(crate) mouse: crate::mouse::Mouse,
    pub(crate) clock: clock::Clock,
    indos: indos::InDos,
    pub(crate) check_break: Cell<bool>,
    console_input: ConsoleInput,
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
        mouse: crate::mouse::Mouse,
    ) -> Self {
        let devices = DeviceChain::default();
        let console_input = ConsoleInput::new(keyboard.clone());
        devices.install(Unavailable(*b"PRN     "));
        devices.install(Unavailable(*b"AUX     "));
        devices.install(BiosConsole::new(memory.clone(), console_input.clone()));
        devices.install(NullDevice);
        Self {
            mouse,
            clock: clock::Clock::default(),
            indos: indos::InDos::new(memory),
            check_break: Cell::new(false),
            console_input,
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

    pub(crate) fn control_break(&self) {
        self.console_input.control_break();
    }

    pub(crate) fn indos(&self) -> &indos::InDos {
        &self.indos
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

    pub(crate) fn start_process(&self, psp_segment: u16) {
        self.mouse.retire_callback();
        *self.process.borrow_mut() = Some(Rc::new(Process::new(
            psp_segment,
            self.devices.clone(),
            &self.stdio,
        )));
    }
}
