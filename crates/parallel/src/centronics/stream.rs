//! The consumer and direct-producer ends of the forward Centronics connection.

use super::{Centronics, ControlEvent, Controls, Status};
use futures_io::{AsyncRead, AsyncWrite};
use std::{
    cell::RefCell,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};

/// Peripheral-owned byte reader. An idle connection waits; it does not report EOF.
/// Session shutdown lets a pending byte drain, then returns EOF. Dropping this
/// reader marks the device offline, discards pending data and fails direct writes.
/// INIT resets the port handshake without ending this stream.
pub struct Input {
    pub(super) device: Rc<RefCell<Centronics>>,
}

impl Input {
    /// Updates device readiness and status independently of available capacity.
    pub fn set_status(&self, status: Status) {
        self.device.borrow_mut().set_status(status);
    }

    pub fn controls(&self) -> Controls {
        self.device.borrow().controls()
    }

    /// Registers a nonblocking handler and supplies the current controls, plus
    /// Initialize if INIT is asserted. Later events preserve each control change
    /// and INIT assertion. The handler must not reenter the connection or port.
    pub fn on_control(&self, handler: impl FnMut(ControlEvent) + 'static) {
        self.device.borrow_mut().on_control(handler);
    }
}

impl AsyncRead for Input {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        self.device.borrow_mut().poll_read(cx, bytes)
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        self.device.borrow_mut().close_reader();
    }
}

/// Direct software output to the same forward stream as port transfers.
///
/// This is the producer end, not a reverse channel from peripheral to guest.
/// Writes accept at most one byte, waiting for capacity and device readiness.
/// They do not change data/control latches or synthesize STROBE/ACK. Flushing
/// waits for direct output to be read. Closing this writer flushes it and prevents
/// further direct writes; the port remains connected and the reader remains open.
pub struct Output {
    pub(super) device: Rc<RefCell<Centronics>>,
    pub(super) closed: bool,
}

impl Output {
    pub fn ready(&self) -> bool {
        !self.closed && self.device.borrow().ready()
    }
}

impl AsyncWrite for Output {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if self.closed {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        self.device.borrow_mut().poll_write(cx, bytes)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.device.borrow_mut().poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.closed {
            return Poll::Ready(Ok(()));
        }
        match self.as_mut().poll_flush(cx) {
            Poll::Ready(Ok(())) => {
                self.closed = true;
                Poll::Ready(Ok(()))
            }
            result => result,
        }
    }
}
