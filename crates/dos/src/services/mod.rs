//! DOS interrupt dispatch and service handlers.

use relocade_machine::{real_mode_interrupt::interrupt, MemoryView};

mod clock;
pub(crate) mod console;
mod control;
mod directories;
mod dispatch;
pub(crate) use dispatch::{int20, int21_entry};
mod files;
mod io;
mod ioctl;
mod memory;
mod search;
mod system;
pub(crate) use system::absolute_disk_unavailable_entry;

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

    #[interrupt(context(memory))]
    fn exit(&self, memory: &mut MemoryView<'_>, status: Result<u8, ServiceError>) {
        self.dos.mouse.end_process();
        self.process
            .terminate(status, memory, &mut self.dos.filesystem());
        self.dos.request_exit();
    }
}
