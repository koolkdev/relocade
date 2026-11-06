use super::*;
use relocade_machine::{Clock, DeviceConnection};
use relocade_vga::{Scanout, Vga};
use std::{
    cell::{Cell, RefCell},
    future::{poll_fn, Future},
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
struct ManualClock {
    now: Cell<Duration>,
    waker: RefCell<Option<Waker>>,
}

impl ManualClock {
    fn set(&self, now: Duration) {
        self.now.set(now);
        if let Some(waker) = self.waker.borrow_mut().take() {
            waker.wake();
        }
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        self.now.get()
    }
    async fn wait_until(&self, deadline: Duration) {
        poll_fn(|cx| {
            if self.now() >= deadline {
                Poll::Ready(())
            } else {
                *self.waker.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}

#[derive(Default)]
struct Wakes(AtomicUsize);

impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn device(clock: Rc<ManualClock>) -> (Rc<RefCell<Vga<ManualClock>>>, IoBus) {
    let vga = Rc::new(RefCell::new(Vga::with_clock(clock)));
    let io = IoBus::new([DeviceConnection {
        device: vga.clone(),
        mmio: vec![relocade_vga::MEMORY_RANGE],
        ports: relocade_vga::PORT_RANGES.into_iter().collect(),
    }]);
    for (register, value) in [(1, 640), (2, 480), (3, 8), (4, 1), (6, 1024)] {
        write_register(&io, register, value);
    }
    (vga, io)
}

fn boundary(frame: u64) -> Duration {
    Duration::from_nanos(((490 + (frame - 1) * 525) * 800 * 1_000_000_000).div_ceil(25_175_000))
}

#[test]
fn retrace_wait_preserves_completed_capture_with_monochrome_status_routing() {
    let clock = Rc::new(ManualClock::default());
    let (vga, io) = device(clock.clone());
    io.write_port(0x3c2, 1, 2); // Monochrome status port; keep CPU memory enabled.
    io.write_mmio(0xa0000, 1, 1);
    write_register(&io, 5, 7);
    io.write_mmio(0xa8020, 1, 2); // Pixel (32, 480) with pitch 1024.
    let frames = vga.borrow().frame_source();
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    let mut request = pin!(display_start(&io, &vga, 0x80, 32, 480));
    assert!(request.as_mut().poll(&mut cx).is_pending());
    assert_eq!(active(&io).unwrap().origin(), [0, 0]);
    assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
    clock.set(boundary(1));
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert_eq!(request.as_mut().poll(&mut cx), Poll::Ready(Some([32, 480])));
    let completed = frames.current_capture().unwrap();
    assert_eq!(completed.sequence, 1);
    let Scanout::Svga(old) = completed.display.scanout() else {
        panic!("packed capture")
    };
    assert_eq!(old.origin(), [0, 0]);
    assert_eq!(completed.display.vram_byte(0), Some(1));
    clock.set(boundary(2));
    io.read_port(0x3ba, 1);
    let next = frames.current_capture().unwrap();
    assert_eq!(next.sequence, 2);
    let Scanout::Svga(layout) = next.display.scanout() else {
        panic!("packed capture")
    };
    assert_eq!(layout.origin(), [32, 480]);
    assert_eq!(next.display.vram_byte(layout.display_start()), Some(2));
    assert_eq!(old.origin(), [0, 0]);
    assert_eq!(read_register(&io, 5), 7);
}

#[test]
fn requests_already_in_retrace_finish_without_yielding() {
    let clock = Rc::new(ManualClock::default());
    let (vga, io) = device(clock.clone());
    clock.set(boundary(1));
    assert_eq!(
        pin!(display_start(&io, &vga, 0x80, 384, 3616))
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Some([384, 3616]))
    );
    assert_eq!(active(&io).unwrap().origin(), [384, 3616]);
}

#[test]
fn retrace_wait_wakes_to_revalidate_mode_and_layout() {
    for (register, value) in [(4, 0), (6, 1280), (8, 1)] {
        let (vga, io) = device(Rc::new(ManualClock::default()));
        let wakes = Arc::new(Wakes::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        let mut request = pin!(display_start(&io, &vga, 0x80, 32, 480));
        assert!(request.as_mut().poll(&mut cx).is_pending());
        write_register(&io, register, value);
        if register == 4 {
            io.write_port(0x3c4, 2, 0x0100); // No future raster event in this mode.
        }
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
        let changed = active(&io);
        assert_eq!(request.as_mut().poll(&mut cx), Poll::Ready(None));
        assert_eq!(active(&io), changed);
    }
    let (vga, _) = device(Rc::new(ManualClock::default()));
    assert_eq!(
        pin!(display_start(&IoBus::default(), &vga, 0x80, 0, 0))
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(None)
    );
}

#[test]
fn routing_changes_rearm_the_wait_and_late_timers_still_complete_it() {
    let clock = Rc::new(ManualClock::default());
    let (vga, io) = device(clock.clone());
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    let mut request = pin!(display_start(&io, &vga, 0x80, 32, 480));
    assert!(request.as_mut().poll(&mut cx).is_pending());
    io.write_port(0x3c2, 1, 2);
    assert_eq!(wakes.0.swap(0, Ordering::SeqCst), 1);
    assert!(request.as_mut().poll(&mut cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
    clock.set(boundary(1) + Duration::from_millis(1));
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert_eq!(io.read_port(0x3ba, 1) & 8, 0);
    assert_eq!(request.as_mut().poll(&mut cx), Poll::Ready(Some([32, 480])));
    assert_eq!(active(&io).unwrap().origin(), [32, 480]);
}
