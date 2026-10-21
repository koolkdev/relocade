//! DOS environment data shared by host APIs and service callbacks.

use crate::{files::Files, process, ServiceError};
use relocade_bios::Keyboard;
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

#[derive(Default)]
pub(crate) struct DosState {
    pub(crate) files: Files,
    pub(crate) dta_segment: u16,
    pub(crate) dta_offset: u16,
    pub(crate) exit_status: Option<Result<u8, ServiceError>>,
}

impl DosState {
    pub(crate) fn reset_process(&mut self, psp_segment: u16) {
        self.files.reset_handles();
        self.dta_segment = psp_segment;
        self.dta_offset = process::COMMAND_TAIL_OFFSET;
        self.exit_status = None;
    }
}

/// A callback handle to the state owned by a [`crate::Dos`] environment.
/// The backend retains this handle alongside its own pending host call.
#[derive(Clone)]
pub struct DosServices {
    state: Rc<RefCell<DosState>>,
    keyboard: Keyboard,
}

impl DosServices {
    pub(crate) fn new(state: Rc<RefCell<DosState>>) -> Self {
        let keyboard = state.borrow().files.stdio.input.clone();
        Self { state, keyboard }
    }

    pub(crate) fn borrow_mut(&self) -> RefMut<'_, DosState> {
        self.state.borrow_mut()
    }
}

impl AsRef<Keyboard> for DosServices {
    fn as_ref(&self) -> &Keyboard {
        &self.keyboard
    }
}
