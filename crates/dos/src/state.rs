//! DOS environment data shared by host APIs and service handlers.

use crate::{files::Files, process, ServiceError};
use std::cell::{Cell, RefCell};

#[derive(Default)]
pub(crate) struct DosState {
    pub(crate) files: RefCell<Files>,
    pub(crate) dta: Cell<(u16, u16)>,
    pub(crate) exit_status: Cell<Option<Result<u8, ServiceError>>>,
}

impl DosState {
    pub(crate) fn reset_process(&self, psp_segment: u16) {
        self.files.borrow_mut().reset_handles();
        self.dta.set((psp_segment, process::COMMAND_TAIL_OFFSET));
        self.exit_status.set(None);
    }
}
