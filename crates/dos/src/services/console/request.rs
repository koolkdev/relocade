//! Retain input and echo destinations before any read or flush can suspend.

use super::input;
use crate::{console::CharacterOutput, files::Handle, services::Services, DosError};
use relocade_machine::{FarPointer, MemoryView};

/// AH=06h only changes ZF when reading; output preserves it.
pub(in crate::services) enum DirectIo {
    Output(u8),
    Input(Option<u8>),
}

pub(in crate::services) struct Input<'a> {
    source: Result<Handle, DosError>,
    output: CharacterOutput<'a>,
}

impl<'a> Input<'a> {
    pub(in crate::services) fn new(services: &Services<'a>) -> Self {
        let source = services.process.files().readable_handle(0);
        let output = services.stdout();
        Self { source, output }
    }

    pub(super) async fn character(&self, echo: bool) -> Result<u8, DosError> {
        let handle = self.source.as_ref().map_err(|error| *error)?;
        let byte = input::read_character(handle).await?;
        if echo {
            let _ = self.output.write_cooked(byte).await;
        }
        Ok(byte)
    }

    pub(super) async fn direct(&self, byte: u8) -> Result<DirectIo, DosError> {
        if byte == 0xff {
            let handle = self.source.as_ref().map_err(|error| *error)?;
            input::try_read_character(handle).await.map(DirectIo::Input)
        } else {
            // Character output has no DOS write-error return.
            let _ = self.output.write(&[byte]).await;
            Ok(DirectIo::Output(byte))
        }
    }

    pub(super) async fn buffered(
        &self,
        memory: &mut MemoryView<'_>,
        buffer: FarPointer,
        capacity: u8,
    ) -> Result<(), DosError> {
        if capacity == 0 {
            return Ok(());
        }
        let handle = self.source.as_ref().map_err(|error| *error)?;
        let bytes = input::read_line(handle, capacity, &self.output).await?;
        let count = (bytes.len() - 1) as u8; // DOS excludes CR from the count.
        memory
            .write_wrapping(buffer.wrapping_add(1), &[count])
            .expect("real-mode line count");
        memory
            .write_wrapping(buffer.wrapping_add(2), &bytes)
            .expect("real-mode line buffer");
        Ok(())
    }
}

pub(super) fn capacity(memory: &MemoryView<'_>, buffer: FarPointer) -> u8 {
    let mut capacity = [0];
    memory
        .read_wrapping(buffer, &mut capacity)
        .expect("real-mode line buffer");
    capacity[0]
}
