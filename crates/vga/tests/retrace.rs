use relocade_clock_test_support::ManualClock;
mod common;
use common::{boundary, crtc, dots, support::svga, svga_boundary};
use relocade_machine::{Device, IoBus};
use relocade_vga::{RetraceEvent, Vga};
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

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(Default)]
struct Wakes(AtomicUsize);

impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn suppressed_retrace_signal_keeps_waiting_while_capture_continues() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let clock = ManualClock::default();
        let mut vga = Vga::with_clock(clock.clock());
        crtc(&mut vga, 0x17, 0x23);
        let (vga, mut video) = common::start_video(vga);
        let frames = vga.borrow().frames();
        let wakes = Arc::new(Wakes::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        let mut wait = pin!(vga.borrow_mut().wait_for_retrace().unwrap());
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        assert_eq!(
            clock.next_deadline().map(|at| clock.elapsed(at)),
            Some(boundary(1)) // The output task still samples internal retraces.
        );
        clock.advance_to(clock.at(boundary(1)));
        assert_eq!(
            vga.borrow_mut().read_port(context.reborrow(), 0x3da, 1) & 8,
            0
        );
        common::service_video(video.as_mut());
        assert!(frames.try_recv().is_ok());
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
        crtc(&mut *vga.borrow_mut(), 0x17, 0xa3);
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
        assert_eq!(
            wait.as_mut().poll(&mut cx),
            Poll::Ready(RetraceEvent::Reprogrammed)
        );
        assert_eq!(
            pin!(vga.borrow_mut().wait_for_retrace().unwrap()).poll(&mut cx),
            Poll::Ready(RetraceEvent::Started)
        );
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn retrace_sleeps_without_device_activity_and_accepts_a_late_timer() {
    let clock = ManualClock::default();
    let vga = Rc::new(RefCell::new(Vga::with_clock(clock.clock())));
    svga::enable(&mut *vga.borrow_mut());
    let io = IoBus::new(vga.clone(), |io| {
        io.map_ports(relocade_vga::PORT_RANGES);
    });
    let mut wait = pin!(vga.borrow_mut().wait_for_retrace().unwrap());
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        clock.next_deadline().map(|at| clock.elapsed(at)),
        Some(svga_boundary(1))
    );
    // Ordinary reads and bank writes must not wake the wait. BIOS queries save
    // and restore the DISPI index this way while the device remains accessible.
    svga::get(&mut *vga.borrow_mut(), 6);
    svga::set(&mut *vga.borrow_mut(), 5, 1);
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
    clock.advance_to(clock.at(svga_boundary(1) + Duration::from_millis(1)));
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert_eq!(io.read_port(0x3da, 1) & 8, 0); // The narrow pulse has already ended.
    assert_eq!(
        wait.as_mut().poll(&mut cx),
        Poll::Ready(RetraceEvent::Started)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mode_changes_invalidate_waits_before_or_after_the_first_poll() {
    for poll_before_change in [false, true] {
        let clock = ManualClock::default();
        let mut vga = Vga::with_clock(clock.clock());
        svga::enable(&mut vga);
        let mut wait = pin!(vga.wait_for_retrace().unwrap());
        let mut cx = Context::from_waker(Waker::noop());
        if poll_before_change {
            assert!(wait.as_mut().poll(&mut cx).is_pending());
        }
        clock.advance_to(clock.at(Duration::from_millis(1)));
        for (register, value) in [(4, 0), (1, 800), (2, 600), (4, 1)] {
            svga::set(&mut vga, register, value);
        }
        clock.advance_to(clock.at(svga_boundary(1)));
        assert_eq!(
            wait.as_mut().poll(&mut cx),
            Poll::Ready(RetraceEvent::Reprogrammed)
        );
        let mut next_wait = pin!(vga.wait_for_retrace().unwrap());
        // 800x600 starts at the mode change, using 1056 dots/line, retrace at
        // line 601 and a 40 MHz clock. The previous mode's deadline is obsolete.
        let next = Duration::from_millis(1) + dots(601 * 1056, 40_000_000);
        assert!(next_wait.as_mut().poll(&mut cx).is_pending());
        assert_eq!(
            clock.next_deadline().map(|at| clock.elapsed(at)),
            Some(next)
        );
        clock.advance_to(clock.at(next + Duration::from_millis(1)));
        assert_eq!(
            next_wait.as_mut().poll(&mut cx),
            Poll::Ready(RetraceEvent::Started)
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn concurrent_waiters_receive_timer_wakes_and_cancel_independently() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let clock = ManualClock::default();
        let mut vga = Vga::with_clock(clock.clock());
        svga::enable(&mut vga);
        let wakes = [Arc::new(Wakes::default()), Arc::new(Wakes::default())];
        let wakers = wakes.clone().map(Waker::from);
        let mut waits = [(); 2].map(|()| Box::pin(vga.wait_for_retrace().unwrap()));
        for (wait, waker) in waits.iter_mut().zip(&wakers) {
            assert!(wait
                .as_mut()
                .poll(&mut Context::from_waker(waker))
                .is_pending());
        }
        clock.advance_to(clock.at(svga_boundary(1)));
        for wake in &wakes {
            assert_eq!(wake.0.load(Ordering::SeqCst), 1);
        }
        for (wait, waker) in waits.iter_mut().zip(&wakers) {
            assert_eq!(
                wait.as_mut().poll(&mut Context::from_waker(waker)),
                Poll::Ready(RetraceEvent::Started)
            );
        }
        clock.advance_to(clock.at(svga_boundary(1) + Duration::from_millis(1)));
        for wake in &wakes {
            wake.0.store(0, Ordering::SeqCst);
        }
        let mut cancelled = Box::pin(vga.wait_for_retrace().unwrap());
        let mut retained = pin!(vga.wait_for_retrace().unwrap());
        assert!(cancelled
            .as_mut()
            .poll(&mut Context::from_waker(&wakers[0]))
            .is_pending());
        assert!(retained
            .as_mut()
            .poll(&mut Context::from_waker(&wakers[1]))
            .is_pending());
        drop(cancelled);
        vga.write_port(context.reborrow(), 0x46e8, 1, 0);
        assert_eq!(wakes[0].0.load(Ordering::SeqCst), 0);
        assert_eq!(wakes[1].0.load(Ordering::SeqCst), 1);
        assert_eq!(
            retained.as_mut().poll(&mut Context::from_waker(&wakers[1])),
            Poll::Ready(RetraceEvent::Reprogrammed)
        );
        assert!(vga.wait_for_retrace().is_none());
    })
}
