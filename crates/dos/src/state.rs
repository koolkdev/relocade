//! DOS environment data and owned service handles.

use crate::{files::FileSystem, process::ProcessState, stdio::Stdio};
use relocade_bios::Keyboard;
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

#[derive(Default)]
pub(crate) struct DosState {
    pub(crate) filesystem: FileSystem,
    pub(crate) stdio: Stdio,
    pub(crate) process: ProcessState,
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
        let keyboard = state.borrow().stdio.input.clone();
        Self { state, keyboard }
    }

    pub(crate) fn input(
        &self,
        handle: u16,
    ) -> Result<crate::files::CharacterInput, crate::error::DosError> {
        self.state
            .borrow()
            .process
            .active()
            .files
            .character_input(&self.keyboard, handle)
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
