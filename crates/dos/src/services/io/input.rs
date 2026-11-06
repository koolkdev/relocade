//! Retained input and echo handles for DOS character and line operations.

use super::wait::{notify_idle, wait_for_input};
use crate::{
    console::{CharacterOutput, LineEditor},
    devices::CONTROL_C,
    files::Handle,
    services::{control::Request, Services},
    DosError,
};
use relocade_machine::CpuContext;
use std::{
    future::{poll_fn, Future},
    ops::ControlFlow,
    pin::pin,
    task::Poll,
};

pub(in crate::services) struct Input<'a> {
    services: &'a Services<'a>,
    source: Result<Handle, DosError>,
    output: CharacterOutput<'a>,
}

impl<'a> Input<'a> {
    pub(in crate::services) fn new(services: &'a Services<'a>) -> Self {
        let source = services.process.files().handle(0).cloned();
        let output = services.stdout();
        Self {
            services,
            source,
            output,
        }
    }

    pub(in crate::services) fn console(services: &'a Services<'a>, source: Handle) -> Self {
        Self {
            services,
            output: CharacterOutput::handle(services.dos.stdio(), Ok(source.clone())),
            source: Ok(source),
        }
    }

    /// Starts a character service attempt. AH=0Ch flushes the retained input
    /// before each attempt, including retries after a returning INT23 handler.
    pub(in crate::services) async fn start(request: &Request<'a>) -> Self {
        let input = Self::new(request.services);
        if request.function == 0x0c {
            if let Ok(source) = &input.source {
                // A closed input or failed flush must not skip the operation.
                let _ = source.flush_input().await;
            }
        }
        input
    }

    /// A checked read also observes CON when stdin is redirected. The returned
    /// Ctrl-C byte lets the operation invoke INT23 and restart its own state.
    pub(in crate::services) async fn character(
        &self,
        call: &mut CpuContext<'_>,
        request: &Request<'_>,
        checked: bool,
    ) -> Result<u8, DosError> {
        let handle = self.source.as_ref().map_err(|error| *error)?;
        handle.check_read_access()?;
        let mut read = pin!(read_character(handle));
        let mut check = pin!(self.break_pending());
        // A pending break check is part of waiting for checked input. A ready
        // break still takes priority over the retained read.
        let input = poll_fn(|cx| {
            if checked {
                match check.as_mut().poll(cx) {
                    Poll::Ready(true) => return Poll::Ready(Ok(CONTROL_C)),
                    Poll::Ready(false) => {}
                    Poll::Pending => return Poll::Pending,
                }
            }
            match read.as_mut().poll(cx) {
                Poll::Ready(Ok(Some(byte))) => return Poll::Ready(Ok(byte)),
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                // An EOF file may grow during the callback. Retry completed
                // reads, while retaining pending device requests.
                Poll::Ready(Ok(None)) => read.set(read_character(handle)),
                Poll::Pending => {}
            }
            check.set(self.break_pending());
            Poll::Pending
        });
        wait_for_input(call, request, input).await
    }

    /// Reads one edited line, including CR, over the retained input and echo
    /// handles. After INT23 returns, Break tells the operation to resolve fresh
    /// handles and capacity; idle callbacks retain this editor and its partial line.
    pub(in crate::services) async fn read_line(
        &self,
        call: &mut CpuContext<'_>,
        request: &mut Request<'_>,
        capacity: u8,
    ) -> Result<ControlFlow<(), Vec<u8>>, DosError> {
        let mut editor = LineEditor::new(capacity, async |byte| {
            self.echo(byte).await;
            Ok::<_, DosError>(())
        });
        loop {
            let byte = self.character(call, request, true).await?;
            if byte == CONTROL_C {
                request.handle_break(call).await;
                return Ok(ControlFlow::Break(()));
            }
            if editor.accept(byte).await? {
                return Ok(ControlFlow::Continue(editor.finish()));
            }
        }
    }

    pub(in crate::services) async fn break_pending(&self) -> bool {
        if check_console_break(self.services).await {
            return true;
        }
        if let Ok(input) = &self.source {
            if input.check_read_access().is_ok()
                && input.peek_character().await.ok().flatten() == Some(CONTROL_C)
            {
                let _ = input.read(&mut [0]).await;
                return true;
            }
        }
        false
    }

    pub(in crate::services) async fn status(
        &self,
        call: &mut CpuContext<'_>,
        request: &Request<'_>,
    ) -> bool {
        let available = match &self.source {
            Ok(input) => input.input_available().await,
            Err(_) => {
                self.services
                    .dos
                    .devices()
                    .console()
                    .input_available()
                    .await
            }
        }
        .unwrap_or(false);
        if !available {
            notify_idle(call, request).await;
        }
        available
    }

    pub(in crate::services) async fn poll_character(
        &self,
        call: &mut CpuContext<'_>,
        request: &Request<'_>,
    ) -> Result<Option<u8>, DosError> {
        let input = self.source.as_ref().map_err(|error| *error)?;
        input.check_read_access()?;
        let byte = try_read_character(input).await?;
        if byte.is_none() {
            notify_idle(call, request).await;
        }
        Ok(byte)
    }

    pub(in crate::services) async fn write_raw(&self, byte: u8) {
        let _ = self.output.write(&[byte]).await;
    }

    pub(in crate::services) async fn write(
        &self,
        call: &mut CpuContext<'_>,
        request: &Request<'_>,
        byte: u8,
    ) {
        self.status(call, request).await;
        self.echo(byte).await;
    }

    pub(in crate::services) async fn echo(&self, byte: u8) {
        let _ = self.output.write_cooked(byte).await;
    }
}

/// Checked character services share restart handling and retain echo until the
/// character completes. An unchecked AH=07h read uses Input::character directly.
pub(in crate::services) async fn read_checked_character(
    call: &mut CpuContext<'_>,
    request: &mut Request<'_>,
    echo: bool,
) -> Result<u8, DosError> {
    loop {
        let input = Input::start(request).await;
        let byte = input.character(call, request, true).await?;
        if byte == CONTROL_C {
            request.handle_break(call).await;
            continue;
        }
        if echo {
            input.echo(byte).await;
        }
        return Ok(byte);
    }
}

/// BREAK ON extends the CON check to disk services without consuming ordinary input.
pub(in crate::services) async fn check_console_break(services: &Services<'_>) -> bool {
    if services.dos.indos().count() != 1 {
        return false;
    }
    let console = services.dos.devices().console();
    if console.peek_character().await.ok().flatten() == Some(CONTROL_C) {
        let _ = console.read_character().await;
        true
    } else {
        false
    }
}

async fn try_read_character(input: &Handle) -> Result<Option<u8>, DosError> {
    match input {
        Handle::File(_) => {
            let mut byte = [0];
            Ok((input.read(&mut byte).await? != 0).then_some(byte[0]))
        }
        Handle::Device(handle) => handle.device.try_read_character().await,
    }
}

async fn read_character(input: &Handle) -> Result<Option<u8>, DosError> {
    if let Handle::Device(handle) = input {
        return handle.device.read_character().await.map(Some);
    }
    try_read_character(input).await
}
