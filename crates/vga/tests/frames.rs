use futures_util::task::{waker, ArcWake};
use relocade_machine::{Clock, Device};
mod common;
use common::{boundary, crtc, poll_video, ManualClock};
use relocade_vga::{Vga, CRTC_INDEX_PORT, TEXT_MEMORY_START};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn tick(clock: &ManualClock, video: Pin<&mut impl Future<Output = ()>>, now: Duration) {
    clock.set(now);
    poll_video(video, &mut Context::from_waker(Waker::noop()));
}

#[derive(Default)]
struct WakeCount(AtomicUsize);
impl ArcWake for WakeCount {
    fn wake_by_ref(this: &Arc<Self>) {
        this.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn ready_deadlines_leave_a_polling_opportunity_for_other_activities() {
    #[derive(Default)]
    struct LateClock {
        now: Cell<Duration>,
        waits: Cell<u32>,
    }
    impl Clock for LateClock {
        fn now(&self) -> Duration {
            self.now.get()
        }
        async fn wait_until(&self, deadline: Duration) {
            let waits = self.waits.get() + 1;
            assert!(
                waits <= 2,
                "video must return before another ready deadline"
            );
            self.waits.set(waits);
            self.now.set(deadline);
        }
    }
    let clock = Rc::new(LateClock::default());
    let vga = Rc::new(RefCell::new(Vga::with_clock(clock.clone())));
    let source = vga.borrow().frame_source();
    let mut video = Box::pin(Vga::run(vga));
    let wakes = Arc::new(WakeCount::default());
    let waker = waker(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(video.as_mut().poll(&mut cx).is_pending());
    assert_eq!(clock.waits.get(), 1);
    assert!(wakes.0.load(Ordering::Relaxed) > 0);
    assert!(video.as_mut().poll(&mut cx).is_pending());
    assert_eq!(source.current_capture().unwrap().time, boundary(1));
    assert_eq!(clock.waits.get(), 2);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn deadlines_capture_coherent_frames_and_coalesce_late_service() {
    let clock = Rc::new(ManualClock::default());
    let vga = Rc::new(RefCell::new(Vga::with_clock(clock.clone())));
    let mut video = Box::pin(Vga::run(vga.clone()));
    let source = vga.borrow().frame_source();
    assert!(source.current_capture().is_none());
    vga.borrow_mut()
        .write_mmio(TEXT_MEMORY_START, 4, 0x2fdb_1edb);
    crtc(&mut *vga.borrow_mut(), 0x0f, 1);
    tick(&clock, video.as_mut(), boundary(1));
    let first = source.current_capture().unwrap();
    assert_eq!(first.time, boundary(1));
    let first_pixels = first.rasterize();
    assert_eq!((first_pixels.width, first_pixels.height), (640, 200));
    assert_eq!(first_pixels.pixel_aspect_ratio, (5, 12));
    assert_eq!(first_pixels.pixels.len(), 640 * 200 * 4);
    assert_eq!(first.sequence, 1);
    assert_eq!(&first_pixels.pixels[..4], [255, 255, 85, 255]);
    assert_eq!(&first_pixels.pixels[32..36], [255, 255, 255, 255]);

    vga.borrow_mut()
        .write_mmio(TEXT_MEMORY_START, 1, u64::from(b' '));
    vga.borrow_mut().write_port(CRTC_INDEX_PORT, 2, 0x010d);
    tick(
        &clock,
        video.as_mut(),
        boundary(2) - Duration::from_nanos(1),
    );
    assert!(Arc::ptr_eq(&first, &source.current_capture().unwrap()));
    tick(&clock, video.as_mut(), boundary(2));
    let second = source.current_capture().unwrap();
    assert_eq!(second.time, boundary(2));
    assert_eq!(second.sequence, 2);
    assert_eq!(&second.rasterize().pixels[..4], [255, 255, 255, 255]);
    assert_eq!(&first_pixels.pixels[..4], [255, 255, 85, 255]);

    tick(
        &clock,
        video.as_mut(),
        boundary(100) + Duration::from_micros(1),
    );
    assert_eq!(source.current_capture().unwrap().time, boundary(100));
    vga.borrow_mut()
        .write_mmio(TEXT_MEMORY_START + 2, 1, u64::from(b'X'));
    drop(video);
    drop(vga);
    assert_eq!(
        &source.current_capture().unwrap().rasterize().pixels[..4],
        [255, 255, 255, 255]
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn consumers_wake_independently_skip_old_frames_and_finish_on_close() {
    let clock = Rc::new(ManualClock::default());
    let vga = Rc::new(RefCell::new(Vga::with_clock(clock.clone())));
    let mut video = Box::pin(Vga::run(vga.clone()));
    let source = vga.borrow().frame_source();
    let wake_a = Arc::new(WakeCount::default());
    let wake_b = Arc::new(WakeCount::default());
    let waker_a = waker(wake_a.clone());
    let waker_b = waker(wake_b.clone());
    let mut cx_a = Context::from_waker(&waker_a);
    let mut cx_b = Context::from_waker(&waker_b);
    let mut a = Box::pin(source.next_capture(None));
    let mut b = Box::pin(source.next_capture(None));
    assert!(a.as_mut().poll(&mut cx_a).is_pending());
    assert!(b.as_mut().poll(&mut cx_b).is_pending());
    tick(&clock, video.as_mut(), boundary(1));
    assert!(wake_a.0.load(Ordering::Relaxed) > 0);
    assert!(wake_b.0.load(Ordering::Relaxed) > 0);
    assert!(matches!(a.as_mut().poll(&mut cx_a), Poll::Ready(Some(_))));
    assert!(matches!(b.as_mut().poll(&mut cx_b), Poll::Ready(Some(_))));

    for index in 2..=20 {
        tick(&clock, video.as_mut(), boundary(index));
    }
    let mut slow = Box::pin(source.next_capture(Some(0)));
    let Poll::Ready(Some(latest)) = slow.as_mut().poll(&mut cx_a) else {
        panic!("latest frame should be ready");
    };
    assert_eq!(latest.time, boundary(20));
    let mut next = Box::pin(source.next_capture(Some(latest.sequence)));
    assert!(next.as_mut().poll(&mut cx_a).is_pending());
    let wakes = wake_a.0.load(Ordering::Relaxed);
    drop(video);
    drop(vga);
    assert!(wake_a.0.load(Ordering::Relaxed) > wakes);
    assert!(matches!(next.as_mut().poll(&mut cx_a), Poll::Ready(None)));
    assert!(Arc::ptr_eq(&latest, &source.current_capture().unwrap()));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn video_time_animates_unchanged_memory_and_retained_pixels_stay_immutable() {
    let clock = Rc::new(ManualClock::default());
    let vga = Rc::new(RefCell::new(Vga::with_clock(clock.clone())));
    let mut video = Box::pin(Vga::run(vga.clone()));
    vga.borrow_mut().write_mmio(TEXT_MEMORY_START, 2, 0x94db);
    vga.borrow_mut()
        .write_mmio(TEXT_MEMORY_START + 2, 2, 0x0720);
    crtc(&mut *vga.borrow_mut(), 0x0f, 1);
    vga.borrow_mut().write_port(CRTC_INDEX_PORT, 2, 0x060a);
    vga.borrow_mut().write_port(CRTC_INDEX_PORT, 2, 0x070b);
    let source = vga.borrow().frame_source();
    tick(&clock, video.as_mut(), boundary(1));
    let on = source.current_capture().unwrap();
    let pixels = on.rasterize().pixels.to_vec();
    let cursor = (6 * 640 + 8) * 4;
    assert_eq!(&pixels[..4], [170, 0, 0, 255]);
    assert_eq!(&pixels[cursor..cursor + 4], [170, 170, 170, 255]);
    tick(&clock, video.as_mut(), boundary(16)); // Sixteen vertical retraces toggle the blink phase.
    let off = source.current_capture().unwrap();
    let off_pixels = off.rasterize();
    let blink = &off_pixels.pixels;
    assert_eq!(&blink[..4], [0, 0, 170, 255]);
    assert_eq!(&blink[cursor..cursor + 4], [0, 0, 0, 255]);
    assert_eq!(&*on.rasterize().pixels, pixels);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn writes_preserve_overdue_output_and_screen_off_blanks_only_later_frames() {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    let source = vga.frame_source();
    vga.write_mmio(TEXT_MEMORY_START, 2, 0x1edb);
    crtc(&mut vga, 0x0a, 0x20);
    clock.set(boundary(5) + Duration::from_micros(1));
    vga.write_mmio(TEXT_MEMORY_START, 2, 0x2fdb);
    let before = source.current_capture().unwrap();
    assert_eq!(before.sequence, 5);
    assert_eq!(before.time, boundary(5));
    assert_eq!(&before.rasterize().pixels[..4], [255, 255, 85, 255]);
    vga.write_port(0x3c4, 2, 0x2101);
    clock.set(boundary(6));
    vga.read_port(0x3da, 1);
    let blank = source.current_capture().unwrap();
    assert!(blank
        .rasterize()
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [0, 0, 0, 255]));
    vga.write_port(0x3c4, 2, 0x0101);
    clock.set(boundary(7));
    vga.read_port(0x3da, 1);
    assert_eq!(
        &source.current_capture().unwrap().rasterize().pixels[..4],
        [255, 255, 255, 255]
    );
    assert_eq!(&before.rasterize().pixels[..4], [255, 255, 85, 255]);
}
