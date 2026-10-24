//! DOS environment data and owned service handles.

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

/// Owned access to the services of a [`crate::Dos`] environment.
/// Pending operations retain the input records they need without borrowing DOS state.
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

    pub(crate) fn input(
        &self,
        handle: u16,
    ) -> Result<crate::files::CharacterInput, crate::error::DosError> {
        self.state.borrow().files.character_input(handle)
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
