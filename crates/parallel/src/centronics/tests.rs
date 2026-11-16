use super::*;
use crate::Port;
use futures_io::{AsyncRead, AsyncWrite};
use relocade_clock_test_support::ManualClock;
use relocade_machine::{Device, IoBus};
use relocade_pic::Pic;
use std::{
    cell::RefCell,
    future::Future,
    io,
    pin::{pin, Pin},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(Default)]
struct Woken(AtomicBool);

impl Wake for Woken {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct Hardware {
    clock: ManualClock,
    io: IoBus,
    input: Input,
    output: Output,
}

impl Hardware {
    fn new() -> Self {
        let clock = ManualClock::default();
        let pic = Pic::default();
        let port = Rc::new(RefCell::new(Port::new(0x378, pic.irq(7, false))));
        let pic = Rc::new(RefCell::new(pic));
        let io = IoBus::new(pic.clone(), |io| Pic::configure_io(io, &pic));
        io.connect(port.clone(), |io| Port::configure_io(io, &port));
        io.write_port(0x37a, 1, 0x0c);
        let (input, output) = Centronics::attach(&mut port.borrow_mut(), clock.clock());
        Self {
            clock,
            io,
            input,
            output,
        }
    }

    fn strobe(&self, byte: u8) {
        self.io.write_port(0x378, 1, u32::from(byte));
        self.io.write_port(0x37a, 1, 0x0d);
        self.io.write_port(0x37a, 1, 0x0c);
    }

    fn irq(&self) -> Poll<u8> {
        self.io
            .poll_interrupt(&mut Context::from_waker(Waker::noop()))
    }
}

fn read(input: &mut Input, cx: &mut Context<'_>) -> Poll<Option<u8>> {
    let mut bytes = [0; 4];
    match Pin::new(input).poll_read(cx, &mut bytes) {
        Poll::Ready(Ok(1)) => Poll::Ready(Some(bytes[0])),
        Poll::Ready(Ok(0)) => Poll::Ready(None),
        Poll::Pending => Poll::Pending,
        result => panic!("unexpected read: {result:?}"),
    }
}

fn write(output: &mut Output, cx: &mut Context<'_>, bytes: &[u8]) -> Poll<usize> {
    Pin::new(output).poll_write(cx, bytes).map(Result::unwrap)
}

fn close(output: &mut Output, cx: &mut Context<'_>) -> Poll<()> {
    Pin::new(output).poll_close(cx).map(Result::unwrap)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn strobe_wakes_the_reader_and_preserves_the_byte_until_consumed() {
    let mut hardware = Hardware::new();
    let flag = Arc::new(Woken::default());
    let waker = Waker::from(flag.clone());
    let mut cx = Context::from_waker(&waker);
    let mut activity = pin!(Centronics::run(hardware.input.device.clone()));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Pending);
    hardware.io.write_port(0x378, 1, 0x1b);
    hardware.io.write_port(0x37a, 1, 0x0d);
    hardware.io.write_port(0x37a, 1, 0x0d);
    hardware.io.write_port(0x378, 1, 0xff);
    hardware.io.write_port(0x37a, 1, 0x0c);
    assert!(flag.0.swap(false, Ordering::SeqCst));
    assert_eq!(hardware.io.read_port(0x379, 1), 0x5f);
    let empty = Pin::new(&mut hardware.input).poll_read(&mut cx, &mut []);
    assert_eq!(empty.map(Result::unwrap), Poll::Ready(0));
    hardware.io.write_port(0x37a, 1, 0x0d); // Busy strobes are ignored.
    hardware.clock.advance_by(Duration::from_millis(1));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(hardware.io.read_port(0x379, 1), 0x5f); // No ACK before reading.
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(Some(0x1b)));
    assert_eq!(hardware.io.read_port(0x379, 1), 0x1f);
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hardware.clock.advance_by(Duration::from_micros(5));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(hardware.io.read_port(0x379, 1), 0xdf);
    hardware.io.write_port(0x37a, 1, 0x0d); // A held strobe cannot retry.
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Pending);
    hardware.io.write_port(0x37a, 1, 0x0c);
    hardware.io.write_port(0x37a, 1, 0x0d);
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(Some(0xff)));
    assert_eq!(hardware.io.read_port(0x379, 1), 0x5f); // STROBE still asserted.
    hardware.io.write_port(0x37a, 1, 0x0c);
    assert_eq!(hardware.io.read_port(0x379, 1), 0x1f);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn direct_output_and_port_transfers_share_capacity_and_order() {
    let mut hardware = Hardware::new();
    let mut cloned_output = hardware.output.clone();
    let flag = Arc::new(Woken::default());
    let waker = Waker::from(flag.clone());
    let mut cx = Context::from_waker(&waker);
    let mut activity = pin!(Centronics::run(hardware.input.device.clone()));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(write(&mut hardware.output, &mut cx, b"AB"), Poll::Ready(1));
    assert_eq!(hardware.io.read_port(0x378, 1), 0);
    assert_eq!(hardware.io.read_port(0x37a, 1), 0xec);
    assert_eq!(hardware.io.read_port(0x379, 1), 0x5f); // BUSY without ACK.
    assert!(!hardware.output.ready());
    assert!(!cloned_output.ready());
    assert!(write(&mut cloned_output, &mut cx, b"B").is_pending());
    flag.0.store(false, Ordering::SeqCst);
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(Some(b'A')));
    assert!(flag.0.swap(false, Ordering::SeqCst));
    assert_eq!(write(&mut cloned_output, &mut cx, b"B"), Poll::Ready(1));
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(Some(b'B')));
    hardware.strobe(b'C');
    assert!(write(&mut hardware.output, &mut cx, b"D").is_pending());
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(Some(b'C')));
    assert!(write(&mut hardware.output, &mut cx, b"D").is_pending());
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hardware.clock.advance_by(Duration::from_micros(5));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(write(&mut hardware.output, &mut cx, b"D"), Poll::Ready(1));
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(Some(b'D')));
    assert_eq!(hardware.io.read_port(0x378, 1), u32::from(b'C'));
    assert_eq!(hardware.io.read_port(0x37a, 1), 0xec);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn init_cancels_port_transfers_but_preserves_accepted_direct_output() {
    let mut hardware = Hardware::new();
    let events = Rc::new(RefCell::new(Vec::new()));
    hardware.input.on_control({
        let events = events.clone();
        move |event| events.borrow_mut().push(event)
    });
    let mut activity = pin!(Centronics::run(hardware.input.device.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hardware.strobe(b'A');
    hardware.io.write_port(0x37a, 1, 0x08);
    hardware.io.write_port(0x37a, 1, 0x08);
    hardware.io.write_port(0x37a, 1, 0x0c);
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Pending);
    assert_eq!(write(&mut hardware.output, &mut cx, b"B"), Poll::Ready(1));
    hardware.io.write_port(0x37a, 1, 0x08);
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(Some(b'B')));
    hardware.io.write_port(0x37a, 1, 0x0e);
    hardware.io.write_port(0x378, 1, 0x0d);
    hardware.io.write_port(0x37a, 1, 0x0f);
    hardware.io.write_port(0x37a, 1, 0x0e);
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(Some(0x0d)));
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Pending); // AUTOFEED adds no byte.
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hardware.io.write_port(0x37a, 1, 0x0a); // Cancel the ACK timer.
    hardware.io.write_port(0x37a, 1, 0x0e);
    hardware.io.write_port(0x378, 1, 0x42);
    hardware.io.write_port(0x37a, 1, 0x0f);
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(Some(0x42)));
    hardware.clock.advance_by(Duration::from_micros(5));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(hardware.io.read_port(0x379, 1), 0x5f); // Still-held STROBE.
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| **event == ControlEvent::Initialize)
            .count(),
        3
    );
    assert_eq!(
        hardware.input.controls(),
        Controls {
            select: true,
            auto_feed: true,
        }
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn readiness_gates_both_paths_and_host_controls_only_gate_port_transfers() {
    let mut hardware = Hardware::new();
    let mut cx = Context::from_waker(Waker::noop());
    hardware.input.set_status(Status {
        ready: false,
        selected: false,
        paper_empty: true,
        error: true,
    });
    assert_eq!(hardware.io.read_port(0x379, 1), 0x67);
    hardware.strobe(b'A');
    assert!(write(&mut hardware.output, &mut cx, b"B").is_pending());
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Pending);
    hardware.input.set_status(Status::default());
    hardware.io.write_port(0x37a, 1, 4);
    hardware.io.write_port(0x37a, 1, 5); // SELECTIN inactive.
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Pending);
    hardware.io.write_port(0x37a, 1, 8);
    hardware.io.write_port(0x37a, 1, 9); // INIT asserted.
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Pending);
    assert_eq!(write(&mut hardware.output, &mut cx, b"B"), Poll::Ready(1));
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(Some(b'B')));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn acknowledgement_completes_and_interrupts_without_guest_io() {
    let mut hardware = Hardware::new();
    let mut activity = pin!(Centronics::run(hardware.input.device.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hardware.io.write_port(0x21, 1, 0x7f);
    hardware.io.write_port(0x37a, 1, 0x1c);
    assert_eq!(hardware.irq(), Poll::Ready(0x0f));
    hardware.io.write_port(0x20, 1, 0x20);
    hardware.io.write_port(0x21, 1, 0xff);
    hardware.io.write_port(0x378, 1, 0xa5);
    hardware.io.write_port(0x37a, 1, 0x1d);
    hardware.io.write_port(0x37a, 1, 0x1c);
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(Some(0xa5)));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hardware.clock.advance_by(Duration::from_micros(4));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(hardware.irq(), Poll::Pending);
    hardware.clock.advance_by(Duration::from_micros(1));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(hardware.irq(), Poll::Pending);
    hardware.io.write_port(0x21, 1, 0x7f);
    assert_eq!(hardware.irq(), Poll::Ready(0x0f));
    hardware.io.write_port(0x20, 1, 0x20);
    assert_eq!(hardware.irq(), Poll::Pending);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn close_flushes_direct_output_while_session_shutdown_ends_the_reader() {
    let mut hardware = Hardware::new();
    let mut cloned_output = hardware.output.clone();
    let flag = Arc::new(Woken::default());
    let waker = Waker::from(flag.clone());
    let mut cx = Context::from_waker(&waker);
    // Own the future so dropping it ends the session before draining the reader.
    let mut activity = Box::pin(Centronics::run(hardware.input.device.clone()));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(write(&mut hardware.output, &mut cx, b"A"), Poll::Ready(1));
    assert!(close(&mut hardware.output, &mut cx).is_pending());
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(Some(b'A')));
    assert_eq!(close(&mut hardware.output, &mut cx), Poll::Ready(()));
    let failed = Pin::new(&mut hardware.output).poll_write(&mut cx, b"B");
    assert!(matches!(failed, Poll::Ready(Err(e)) if e.kind() == io::ErrorKind::BrokenPipe));
    assert!(!hardware.output.clone().ready());
    assert!(cloned_output.ready());
    assert_eq!(write(&mut cloned_output, &mut cx, b"B"), Poll::Ready(1));
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(Some(b'B')));
    hardware.strobe(b'C');
    drop(activity);
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(Some(b'C')));
    assert_eq!(read(&mut hardware.input, &mut cx), Poll::Ready(None));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dropping_the_reader_wakes_and_fails_pending_direct_output() {
    let mut hardware = Hardware::new();
    let flag = Arc::new(Woken::default());
    let waker = Waker::from(flag.clone());
    let mut cx = Context::from_waker(&waker);
    assert_eq!(write(&mut hardware.output, &mut cx, b"A"), Poll::Ready(1));
    assert!(write(&mut hardware.output, &mut cx, b"B").is_pending());
    drop(hardware.input);
    assert!(flag.0.swap(false, Ordering::SeqCst));
    let failed = Pin::new(&mut hardware.output).poll_write(&mut cx, b"B");
    assert!(matches!(failed, Poll::Ready(Err(e)) if e.kind() == io::ErrorKind::BrokenPipe));
    assert_eq!(hardware.io.read_port(0x379, 1), 0x47);
    hardware.io.write_port(0x37a, 1, 8);
    hardware.io.write_port(0x37a, 1, 0x0c);
    assert!(!hardware.output.ready());
}
