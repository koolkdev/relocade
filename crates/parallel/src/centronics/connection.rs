//! Owns transfers, connector status and wakeups for both byte producers.

use super::{ControlEvent, Controls, Input, Output, Status};
use crate::{InputPins, Inputs, OutputPins, Port};
use relocade_machine::{Clock, Device, Runner, Sleep, Timestamp};
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

const ACK_DURATION: Duration = Duration::from_micros(5);

enum Transfer {
    Idle,
    Direct(u8),
    Strobed { byte: u8, released: bool },
    Accepted,
    Acknowledging { deadline: Timestamp, wait: Sleep },
}

/// A forward byte connection over the generic parallel-port signals.
///
/// [`Self::connect`] returns an [`Input`] implementing `futures_io::AsyncRead`
/// for the peripheral and an [`Output`] implementing `futures_io::AsyncWrite`
/// for direct software output. Both producers share one pending-byte slot.
/// Direct output bypasses register writes, STROBE and ACK, while sharing device
/// readiness, capacity and byte ordering with port transfers.
///
/// A falling STROBE captures one byte and asserts BUSY. ACK starts after the
/// peripheral reads that byte and STROBE is released, then lasts five emulated
/// microseconds. Busy strobes are ignored. INIT cancels an unread port transfer
/// and the handshake, but preserves bytes already accepted by direct output.
/// Sideband callbacks let the peripheral apply its own initialization policy.
/// There is no printer-language interpretation or reverse byte channel.
pub struct Centronics {
    clock: Clock,
    inputs: Option<Inputs>,
    outputs: OutputPins,
    status: Status,
    transfer: Transfer,
    reader_open: bool,
    active: bool,
    control_handler: Option<Box<dyn FnMut(ControlEvent)>>,
    reader_waker: Option<Waker>,
    output_wakers: Vec<Waker>,
    activity_waker: Option<Waker>,
}

impl Centronics {
    /// Attaches one connection and starts its ACK activity on the session.
    /// The returned reader is owned by the peripheral. The writer can be given
    /// to a higher-level service such as a DOS LPT character driver.
    pub async fn connect(runner: &Runner, port: &Rc<RefCell<Port>>) -> (Input, Output) {
        let (input, output) = Self::attach(&mut port.borrow_mut(), runner.clock().clone());
        runner.spawn_activity(Self::run(input.device.clone())).await;
        (input, output)
    }

    pub(super) fn attach(port: &mut Port, clock: Clock) -> (Input, Output) {
        let device = Rc::new(RefCell::new(Self {
            clock,
            inputs: None,
            outputs: port.output_pins(),
            status: Status::default(),
            transfer: Transfer::Idle,
            reader_open: true,
            active: true,
            control_handler: None,
            reader_waker: None,
            output_wakers: Vec::new(),
            activity_waker: None,
        }));
        port.attach({
            let device = device.clone();
            move |outputs, inputs| {
                let mut device = device.borrow_mut();
                device.inputs = Some(inputs.clone());
                device.output_changed(outputs);
            }
        });
        (
            Input {
                device: device.clone(),
            },
            Output {
                device,
                closed: false,
            },
        )
    }

    pub(super) fn controls(&self) -> Controls {
        Controls {
            select: !self.outputs.select_in_n,
            auto_feed: !self.outputs.auto_feed_n,
        }
    }

    pub(super) fn ready(&self) -> bool {
        self.active
            && self.reader_open
            && self.status.ready
            && matches!(self.transfer, Transfer::Idle)
    }

    fn output_changed(&mut self, outputs: OutputPins) {
        self.refresh_ack();
        let previous = self.outputs;
        self.outputs = outputs;
        if previous.select_in_n != outputs.select_in_n
            || previous.auto_feed_n != outputs.auto_feed_n
        {
            self.notify_control(ControlEvent::Changed(self.controls()));
        }
        if previous.initialize_n && !outputs.initialize_n {
            // Direct writes already reported acceptance to their caller.
            if !matches!(self.transfer, Transfer::Direct(_)) {
                self.set_transfer(Transfer::Idle);
            }
            self.notify_control(ControlEvent::Initialize);
        }
        if outputs.initialize_n
            && !outputs.select_in_n
            && previous.strobe_n
            && !outputs.strobe_n
            && self.ready()
        {
            self.set_transfer(Transfer::Strobed {
                byte: outputs.data,
                released: false,
            });
        }
        if !previous.strobe_n && outputs.strobe_n {
            match &mut self.transfer {
                Transfer::Strobed { released, .. } => *released = true,
                Transfer::Accepted => self.acknowledge(),
                _ => {}
            }
        }
        self.publish_status();
        self.wake_activity();
    }

    fn notify_control(&mut self, event: ControlEvent) {
        if let Some(handler) = &mut self.control_handler {
            handler(event);
        }
    }

    fn acknowledge(&mut self) {
        let deadline = self.clock.deadline_after(self.clock.now(), ACK_DURATION);
        self.set_transfer(Transfer::Acknowledging {
            deadline,
            wait: self.clock.sleep_until(deadline),
        });
        self.wake_activity();
    }

    fn refresh_ack(&mut self) {
        if let Transfer::Acknowledging { deadline, .. } = &self.transfer {
            if self.clock.now() >= *deadline {
                self.set_transfer(Transfer::Idle);
            }
        }
    }

    fn wake_reader(&mut self) {
        if let Some(waker) = self.reader_waker.take() {
            waker.wake();
        }
    }

    fn wait_for_output(&mut self, cx: &Context<'_>) {
        if !self
            .output_wakers
            .iter()
            .any(|waker| waker.will_wake(cx.waker()))
        {
            self.output_wakers.push(cx.waker().clone());
        }
    }

    fn wake_activity(&mut self) {
        if let Some(waker) = self.activity_waker.take() {
            waker.wake();
        }
    }

    /// Publishes connector status and notifies direct writers of the state change.
    fn publish_status(&mut self) {
        let connected = self.active && self.reader_open;
        self.inputs
            .as_ref()
            .expect("attached Centronics connection")
            .set(InputPins {
                busy: !connected
                    || !self.status.ready
                    || !self.outputs.initialize_n
                    || !matches!(self.transfer, Transfer::Idle),
                acknowledge_n: !matches!(self.transfer, Transfer::Acknowledging { .. }),
                paper_end: self.status.paper_empty,
                selected: connected && self.status.selected,
                error_n: connected && !self.status.error,
            });
        for waker in self.output_wakers.drain(..) {
            waker.wake();
        }
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.activity_waker = Some(cx.waker().clone());
        self.refresh_ack();
        if let Transfer::Acknowledging { wait, .. } = &mut self.transfer {
            if Pin::new(wait).poll(cx).is_ready() {
                self.set_transfer(Transfer::Idle);
            }
        }
        Poll::Pending
    }

    pub(super) fn set_status(&mut self, status: Status) {
        self.status = status;
        self.publish_status();
    }

    pub(super) fn on_control(&mut self, mut handler: impl FnMut(ControlEvent) + 'static) {
        handler(ControlEvent::Changed(self.controls()));
        if !self.outputs.initialize_n {
            handler(ControlEvent::Initialize);
        }
        self.control_handler = Some(Box::new(handler));
    }

    pub(super) fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        match self.transfer {
            Transfer::Direct(byte) => {
                bytes[0] = byte;
                self.set_transfer(Transfer::Idle);
            }
            Transfer::Strobed { byte, released } => {
                bytes[0] = byte;
                if !self.active {
                    self.set_transfer(Transfer::Idle);
                } else if released {
                    self.acknowledge();
                } else {
                    self.set_transfer(Transfer::Accepted);
                }
            }
            _ if !self.active => return Poll::Ready(Ok(0)),
            _ => {
                self.reader_waker = Some(cx.waker().clone());
                return Poll::Pending;
            }
        }
        self.reader_waker = None;
        Poll::Ready(Ok(1))
    }

    pub(super) fn poll_write(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if !self.active || !self.reader_open {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if !self.ready() {
            self.wait_for_output(cx);
            return Poll::Pending;
        }
        self.set_transfer(Transfer::Direct(bytes[0]));
        Poll::Ready(Ok(1))
    }

    pub(super) fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.active || !self.reader_open {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if matches!(self.transfer, Transfer::Direct(_)) {
            self.wait_for_output(cx);
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn set_transfer(&mut self, transfer: Transfer) {
        self.transfer = transfer;
        self.publish_status();
        if matches!(
            self.transfer,
            Transfer::Direct(_) | Transfer::Strobed { .. }
        ) {
            self.wake_reader();
        }
    }

    pub(super) fn close_reader(&mut self) {
        self.reader_open = false;
        self.reader_waker = None;
        self.set_transfer(Transfer::Idle);
        self.wake_activity();
    }

    fn end_session(&mut self) {
        self.active = false;
        if matches!(
            self.transfer,
            Transfer::Acknowledging { .. } | Transfer::Accepted
        ) {
            self.set_transfer(Transfer::Idle);
        } else {
            self.publish_status();
        }
        self.wake_reader();
    }
}

impl Device for Centronics {
    async fn run(device: Rc<RefCell<Self>>) {
        struct Session(Rc<RefCell<Centronics>>);
        impl Drop for Session {
            fn drop(&mut self) {
                self.0.borrow_mut().end_session();
            }
        }
        let session = Session(device);
        poll_fn(|cx| session.0.borrow_mut().poll(cx)).await;
    }
}
