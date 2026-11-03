//! DOS interrupt dispatch and service handlers.

use relocade_machine::{real_mode_interrupt::interrupt, MemoryView};

pub(crate) mod console;
mod directories;
mod dispatch;
pub(crate) use dispatch::{int20, int21};
mod files;
mod ioctl;
mod memory;
mod search;
mod system;

use crate::{process::Process, state::DosState, ServiceError};

/// Resources retained for one DOS interrupt invocation, independent of CPU registers.
/// The active process is selected at entry and retained across pending I/O.
pub(crate) struct Services<'call> {
    pub(crate) dos: &'call DosState,
    pub(crate) process: &'call Process,
}

impl<'call> Services<'call> {
    fn stdout(&self) -> crate::console::CharacterOutput<'call> {
        crate::console::CharacterOutput::handle(
            self.dos.stdio(),
            self.process.files().writable_handle(1),
        )
    }

    fn exit(&self, memory: &mut MemoryView<'_>, status: Result<u8, ServiceError>) {
        self.process
            .terminate(status, memory, &mut self.dos.filesystem());
        self.dos.request_exit();
    }
}

#[interrupt(context(memory))]
fn fatal(memory: &mut MemoryView<'_>, services: &Services<'_>, error: ServiceError) {
    services.exit(memory, Err(error));
}
