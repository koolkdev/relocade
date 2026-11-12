use super::*;
use relocade_clock_test_support::ManualClock;
use relocade_machine::{Clock, Device};
use relocade_vga::{DispiAdapter, Scanout};
use std::{
    cell::RefCell,
    future::Future,
    pin::pin,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

#[derive(Default)]
struct Wakes(AtomicUsize);

impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn device(clock: Clock) -> (Rc<RefCell<DispiAdapter>>, IoBus) {
    let card = Rc::new(RefCell::new(DispiAdapter::with_clock(clock)));
    let io = IoBus::new(card.clone(), |io| DispiAdapter::configure_io(io, &card));
    for (register, value) in [(1, 640), (2, 480), (3, 8), (4, 1), (6, 1024)] {
        write_register(&io, register, value);
    }
    (card, io)
}

fn write_register(io: &IoBus, register: u16, value: u16) {
    io.write_port(0x1ce, 2, u32::from(register));
    io.write_port(0x1cf, 2, u32::from(value));
}

fn read_register(io: &IoBus, register: u16) -> u16 {
    let index = io.read_port(0x1ce, 2);
    io.write_port(0x1ce, 2, u32::from(register));
    let value = io.read_port(0x1cf, 2) as u16;
    io.write_port(0x1ce, 2, index);
    value
}

fn boundary(frame: u64) -> Duration {
    Duration::from_nanos(((490 + (frame - 1) * 525) * 800 * 1_000_000_000).div_ceil(25_175_000))
}

#[test]
fn retrace_wait_preserves_completed_capture_with_monochrome_status_routing() {
    let clock = ManualClock::default();
    let (card, io) = device(clock.clock());
    io.write_port(0x3c2, 1, 2); // Monochrome status port; keep CPU memory enabled.
    io.write_mmio(0xa0000, 1, 1);
    write_register(&io, 5, 7);
    io.write_mmio(0xa8020, 1, 2); // Pixel (32, 480) with pitch 1024.
    let frames = card.borrow().frames();
    let mut video = pin!(DispiAdapter::run(card.clone()));
    let mut video_cx = Context::from_waker(Waker::noop());
    assert!(video.as_mut().poll(&mut video_cx).is_pending());
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    let mut request = pin!(display_start(&io, &card, 0x80, 32, 480));
    assert!(request.as_mut().poll(&mut cx).is_pending());
    assert_eq!(active(&card).unwrap().origin(), [0, 0]);
    assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
    clock.advance_to(clock.at(boundary(1)));
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert_eq!(request.as_mut().poll(&mut cx), Poll::Ready(Some([32, 480])));
    assert!(video.as_mut().poll(&mut video_cx).is_pending());
    let completed = frames.try_recv().unwrap();
    let Scanout::Packed(old) = completed.scanout() else {
        panic!("packed capture")
    };
    assert_eq!(old.origin(), [0, 0]);
    assert_eq!(completed.vram_byte(0), Some(1));
    clock.advance_to(clock.at(boundary(2)));
    io.read_port(0x3ba, 1);
    assert!(video.as_mut().poll(&mut video_cx).is_pending());
    let next = frames.try_recv().unwrap();
    let Scanout::Packed(layout) = next.scanout() else {
        panic!("packed capture")
    };
    assert_eq!(layout.origin(), [32, 480]);
    assert_eq!(next.vram_byte(layout.display_start()), Some(2));
    assert_eq!(old.origin(), [0, 0]);
    assert_eq!(read_register(&io, 5), 7);
}

#[test]
fn requests_already_in_retrace_finish_without_yielding() {
    let clock = ManualClock::default();
    let (card, io) = device(clock.clock());
    clock.advance_to(clock.at(boundary(1)));
    assert_eq!(
        pin!(display_start(&io, &card, 0x80, 384, 3616))
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Some([384, 3616]))
    );
    assert_eq!(active(&card).unwrap().origin(), [384, 3616]);
}

#[test]
fn retrace_wait_wakes_to_revalidate_mode_and_layout() {
    for (register, value) in [(4, 0), (6, 1280), (8, 1)] {
        let (card, io) = device(ManualClock::new().clock());
        let wakes = Arc::new(Wakes::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        let mut request = pin!(display_start(&io, &card, 0x80, 32, 480));
        assert!(request.as_mut().poll(&mut cx).is_pending());
        write_register(&io, register, value);
        if register == 4 {
            io.write_port(0x3c4, 2, 0x0100); // No future raster event in this mode.
        }
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
        let changed = active(&card);
        assert_eq!(request.as_mut().poll(&mut cx), Poll::Ready(None));
        assert_eq!(active(&card), changed);
    }
    let (card, io) = device(ManualClock::new().clock());
    io.write_port(0x46e8, 1, 0); // Disabled address decoding also blocks direct firmware access.
    assert_eq!(
        pin!(display_start(&io, &card, 0x80, 0, 0)).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(None)
    );
}

#[test]
fn routing_changes_rearm_the_wait_and_late_timers_still_complete_it() {
    let clock = ManualClock::default();
    let (card, io) = device(clock.clock());
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    let mut request = pin!(display_start(&io, &card, 0x80, 32, 480));
    assert!(request.as_mut().poll(&mut cx).is_pending());
    io.write_port(0x3c2, 1, 2);
    assert_eq!(wakes.0.swap(0, Ordering::SeqCst), 1);
    assert!(request.as_mut().poll(&mut cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
    clock.advance_to(clock.at(boundary(1) + Duration::from_millis(1)));
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert_eq!(io.read_port(0x3ba, 1) & 8, 0);
    assert_eq!(request.as_mut().poll(&mut cx), Poll::Ready(Some([32, 480])));
    assert_eq!(active(&card).unwrap().origin(), [32, 480]);
}
