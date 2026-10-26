//! DOS environment data shared by host APIs and service callbacks.

use crate::{files::FileSystem, stdio::Stdio, Process};
use relocade_bios::Keyboard;
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

#[derive(Default)]
pub(crate) struct DosState {
    pub(crate) filesystem: FileSystem,
    pub(crate) stdio: Stdio,
    pub(crate) process: Option<Rc<Process>>,
}

/// A callback handle to the state owned by a [`crate::Dos`] environment.
/// The CPU retains this handle alongside its pending host call.
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

    pub(crate) fn borrow_mut(&self) -> RefMut<'_, DosState> {
        self.state.borrow_mut()
    }
}

impl AsRef<Keyboard> for DosServices {
    fn as_ref(&self) -> &Keyboard {
        &self.keyboard
    }
}
