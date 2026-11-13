use crate::common::fixture::ConnectedVga;
use crate::common::support::inspection::assert_vga_memory_eq;
use crate::common::support::planes::SnapshotPlanes;
use futures_util::task::{ArcWake, waker};
use relocade_clock_test_support::ManualClock;
use relocade_machine::{Clock, ClockSource, Device, Sleep, Timestamp};
mod common;
use common::support::software;
use common::{boundary, crtc, poll_video};
use relocade_vga::{CRTC_INDEX_PORT, DispiAdapter, TEXT_MEMORY_START};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn tick(clock: &ManualClock, video: Pin<&mut impl Future<Output = ()>>, now: Duration) {
    clock.advance_to(clock.at(now));
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
fn status_polling_captures_overdue_output_before_the_guest_timer_notifies() {
    struct DelayedTimer(ManualClock);
    impl ClockSource for DelayedTimer {
        fn now(&self) -> Timestamp {
            self.0.now()
        }

        fn sleep_until(&self, _deadline: Timestamp) -> Sleep {
            // Guest time advances, but the timer has not delivered its wake yet.
            Sleep::new(std::future::pending())
        }
    }

    let clock = ManualClock::default();
    let (vga, mut video) =
        common::start_video(ConnectedVga::new(Clock::new(DelayedTimer(clock.clone()))));
    let source = vga.device.borrow().frames();
    let wakes = Arc::new(WakeCount::default());
    let waker = waker(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    poll_video(video.as_mut(), &mut cx);

    vga.io.write_mmio(TEXT_MEMORY_START, 1, u64::from(b'A'));
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
    clock.advance_to(clock.at(boundary(1)));
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
    vga.io.write_mmio(TEXT_MEMORY_START, 1, u64::from(b'B'));
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
    vga.io.read_port(0x3da, 1);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert!(source.is_empty());
    let first = vga.snapshot();

    vga.io.write_mmio(TEXT_MEMORY_START, 1, u64::from(b'C'));
    clock.advance_to(clock.at(boundary(2)));
    vga.io.write_mmio(TEXT_MEMORY_START, 1, u64::from(b'D'));
    vga.io.read_port(0x3da, 1);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1, "pending work coalesces");
    assert!(source.is_empty());
    poll_video(video.as_mut(), &mut cx);
    let capture = source.try_recv().unwrap();

    assert_eq!(first.plane_byte(0, 0), b'B');
    assert_eq!(
        capture.plane_byte(0, 0),
        b'D',
        "capture observes writes before the poll"
    );
    assert_eq!(vga.io.read_mmio(TEXT_MEMORY_START, 1), u64::from(b'D'));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn deadlines_capture_coherent_frames_and_coalesce_late_service() {
    let clock = ManualClock::default();
    let vga = common::text_vga(clock.clock());
    let mut video = Box::pin(DispiAdapter::run(vga.device.clone()));
    let source = vga.device.borrow().frames();
    assert!(source.is_empty());
    vga.io.write_mmio(TEXT_MEMORY_START, 4, 0x2fdb_1edb);
    crtc(&vga.io, 0x0f, 1);
    tick(&clock, video.as_mut(), boundary(1));
    let first = source.try_recv().unwrap();
    // An alternate renderer can consume captured state without producing CPU pixels.
    let first_text = common::support::inspection::inspect(&first);
    assert_eq!(first_text.cells[0].character, 0xdb);
    assert_eq!(first_text.cells[0].attribute, 0x1e);
    assert_eq!(first_text.start_address, 0);
    assert_eq!(first_text.cursor.address, 1);
    let first_pixels = software::rasterize(&first);
    assert_eq!((first_pixels.width, first_pixels.height), (640, 200));
    assert_eq!(first_pixels.pixel_aspect_ratio, (5, 12));
    assert_eq!(first_pixels.pixels.len(), 640 * 200 * 4);

    assert_eq!(&first_pixels.pixels[..4], [255, 255, 85, 255]);
    assert_eq!(&first_pixels.pixels[32..36], [255, 255, 255, 255]);

    vga.io.write_mmio(TEXT_MEMORY_START, 1, u64::from(b' '));
    vga.io.write_port(CRTC_INDEX_PORT, 2, 0x010d);
    tick(
        &clock,
        video.as_mut(),
        boundary(2) - Duration::from_nanos(1),
    );
    assert!(source.is_empty());
    tick(&clock, video.as_mut(), boundary(2));
    let second = source.try_recv().unwrap();
    assert_eq!(
        common::support::inspection::inspect(&second).start_address,
        1
    );
    assert_eq!(
        common::support::inspection::inspect(&first).start_address,
        0
    );

    assert_eq!(
        &software::rasterize(&second).pixels[..4],
        [255, 255, 255, 255]
    );
    assert_eq!(&first_pixels.pixels[..4], [255, 255, 85, 255]);

    crtc(&vga.io, 0x0f, 2);
    clock.advance_to(clock.at(boundary(100) + Duration::from_micros(1)));
    let wakes = Arc::new(WakeCount::default());
    let waker = waker(wakes.clone());
    poll_video(video.as_mut(), &mut Context::from_waker(&waker));

    assert_eq!(
        clock.next_deadline().map(|at| clock.elapsed(at)),
        Some(boundary(101))
    );
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
    vga.io.write_mmio(TEXT_MEMORY_START + 2, 1, u64::from(b'X'));
    drop(video);
    drop(vga);
    assert_eq!(
        &software::rasterize(&source.try_recv().unwrap()).pixels[..4],
        [255, 255, 255, 255]
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn receiver_cancels_waits_replaces_queued_frames_and_finishes_on_close() {
    let clock = ManualClock::default();
    let vga = common::text_vga(clock.clock());
    let mut video = Box::pin(DispiAdapter::run(vga.device.clone()));
    let source = vga.device.borrow().frames();
    let wakes = Arc::new(WakeCount::default());
    let waker = waker(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    {
        let mut canceled = std::pin::pin!(source.recv());
        assert!(canceled.as_mut().poll(&mut cx).is_pending());
    }
    let mut first = std::pin::pin!(source.recv());
    assert!(first.as_mut().poll(&mut cx).is_pending());
    tick(&clock, video.as_mut(), boundary(1));
    assert!(wakes.0.load(Ordering::Relaxed) > 0);
    assert!(matches!(first.as_mut().poll(&mut cx), Poll::Ready(Ok(_))));
    assert!(source.is_empty());

    for index in 2..=20 {
        vga.io.write_mmio(TEXT_MEMORY_START, 1, index);
        tick(&clock, video.as_mut(), boundary(index));
        assert_eq!(source.len(), 1);
    }
    drop(video);
    drop(vga);
    assert!(source.is_closed());
    let mut slow = std::pin::pin!(source.recv());
    let Poll::Ready(Ok(latest)) = slow.as_mut().poll(&mut cx) else {
        panic!("latest frame should be ready");
    };
    assert_eq!(latest.plane_byte(0, 0), 20);
    let mut next = std::pin::pin!(source.recv());
    assert!(matches!(next.as_mut().poll(&mut cx), Poll::Ready(Err(_))));
    assert!(source.is_empty());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn refreshes_publish_writes_and_display_changes() {
    let clock = ManualClock::default();
    let vga = common::text_vga(clock.clock());
    let mut video = std::pin::pin!(DispiAdapter::run(vga.device.clone()));
    let source = vga.device.borrow().frames();
    tick(&clock, video.as_mut(), boundary(1));
    let first = source.try_recv().unwrap();
    let wakes = Arc::new(WakeCount::default());
    let change_waker = waker(wakes.clone());
    let mut cx = Context::from_waker(&change_waker);
    let mut changed = std::pin::pin!(source.recv());
    assert!(changed.as_mut().poll(&mut cx).is_pending());

    // Port indices and transfer positions do not change captured display inputs.
    for (port, value) in [
        (0x3c4, 3),
        (0x3ce, 4),
        (0x3d4, 0x13),
        (0x3c8, 42),
        (0x3c0, 0x35),
    ] {
        vga.io.write_port(port, 1, value);
    }
    tick(&clock, video.as_mut(), boundary(2));
    assert_eq!(
        clock.next_deadline().map(|at| clock.elapsed(at)),
        Some(boundary(3))
    );
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
    assert!(changed.as_mut().poll(&mut cx).is_pending());
    assert!(source.is_empty());

    vga.io.write_mmio(TEXT_MEMORY_START, 2, 0x1edb);
    tick(&clock, video.as_mut(), boundary(3));
    tick(&clock, video.as_mut(), boundary(4));
    assert!(wakes.0.load(Ordering::Relaxed) > 0);
    let Poll::Ready(Ok(latest)) = changed.as_mut().poll(&mut cx) else {
        panic!("changed inputs must wake the consumer");
    };

    assert_eq!(latest.plane_byte(0, 0), 0xdb);
    assert_ne!(first.plane_byte(0, 0), 0xdb);
    assert!(source.is_empty());

    let mut changed = std::pin::pin!(source.recv());
    assert!(changed.as_mut().poll(&mut cx).is_pending());
    wakes.0.store(0, Ordering::Relaxed);
    crtc(&vga.io, 0x0f, 1);
    tick(&clock, video.as_mut(), boundary(5));
    assert!(wakes.0.load(Ordering::Relaxed) > 0, "register-only change");
    let Poll::Ready(Ok(latest)) = changed.as_mut().poll(&mut cx) else {
        panic!("changed registers must wake the consumer");
    };
    assert_eq!(latest.vga().unwrap().crtc.values[0x0f], 1);

    // Rewriting identical bytes still publishes a frame without comparing VRAM.
    vga.io.write_mmio(TEXT_MEMORY_START, 2, 0x1edb);
    tick(&clock, video.as_mut(), boundary(6));
    let rewritten = source.try_recv().unwrap();
    assert_eq!(rewritten.plane_byte(0, 0), 0xdb);

    // Frame 8 changes only the cursor phase. Skipping to 24 changes only the
    // attribute phase: the cursor has completed a full cycle in between.
    for (frame, cursor_on, attribute_on) in [(8, false, true), (24, false, false)] {
        let mut changed = std::pin::pin!(source.recv());
        assert!(changed.as_mut().poll(&mut cx).is_pending());
        wakes.0.store(0, Ordering::Relaxed);
        tick(&clock, video.as_mut(), boundary(frame));
        assert!(
            wakes.0.load(Ordering::Relaxed) > 0,
            "blink without guest writes"
        );
        let Poll::Ready(Ok(capture)) = changed.as_mut().poll(&mut cx) else {
            panic!("either blink phase must wake the consumer");
        };

        assert_eq!(capture.vga().unwrap().cursor_blink_on, cursor_on);
        assert_eq!(capture.vga().unwrap().attribute_blink_on, attribute_on);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cpu_access_controls_preserve_display_inputs_and_leave_waiters_asleep() {
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(common::text_vga(clock.clock()));
    let source = vga.device.borrow().frames();
    tick(&clock, video.as_mut(), boundary(1));
    let first = source.try_recv().unwrap();
    let wakes = Arc::new(WakeCount::default());
    let change_waker = waker(wakes.clone());
    let mut cx = Context::from_waker(&change_waker);
    let mut changed = std::pin::pin!(source.recv());
    assert!(changed.as_mut().poll(&mut cx).is_pending());

    for (port, index, value) in [
        (0x3c4, 2, 0x00), // CPU plane mask.
        (0x3c4, 4, 0x0e), // Chain four and odd/even, preserving extended fonts.
        (0x3ce, 0, 0x0f), // Set/reset and CPU read/write controls.
        (0x3ce, 1, 0x0f),
        (0x3ce, 2, 0x0f),
        (0x3ce, 3, 0x1f),
        (0x3ce, 4, 0x03),
        (0x3ce, 5, 0x1b), // Read/write modes without changing serialization.
        (0x3ce, 6, 0x02), // Aperture and CPU odd/even without changing text mode.
        (0x3ce, 7, 0x00),
        (0x3ce, 8, 0x00),
    ] {
        vga.io.write_port(port, 2, index | (value << 8));
        assert_eq!(vga.io.read_port(port + 1, 1), value);
    }
    let inspected = vga.snapshot();
    assert_ne!(
        first.vga(),
        inspected.vga(),
        "raw inspection stays complete"
    );
    assert_eq!(inspected.vga().unwrap().sequencer[2], 0);
    assert_eq!(inspected.vga().unwrap().graphics[5], 0x1b);

    tick(&clock, video.as_mut(), boundary(2));
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
    assert!(changed.as_mut().poll(&mut cx).is_pending());
    assert!(source.is_empty());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mixed_register_display_bits_still_publish_new_inputs() {
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(common::text_vga(clock.clock()));
    let source = vga.device.borrow().frames();
    tick(&clock, video.as_mut(), boundary(1));
    let first = source.try_recv().unwrap();
    assert_eq!(first.vga().unwrap().sequencer[4] & 2, 2);

    for (frame, port, value) in [
        (2, 0x3c4, 0x0004), // Extended font addressing.
        (3, 0x3ce, 0x3005), // Graphics serializer shift, preserving CPU read mode.
        (4, 0x3ce, 0x0f06), // Graphics/text interpretation, preserving CPU mapping.
    ] {
        vga.io.write_port(port, 2, value);
        tick(&clock, video.as_mut(), boundary(frame));
        let capture = source
            .try_recv()
            .expect("display bits must publish a frame");
        assert_vga_memory_eq(&first, &capture);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cursor_and_attribute_blink_animate_independently_and_retained_pixels_stay_immutable() {
    let clock = ManualClock::default();
    let vga = common::text_vga(clock.clock());
    let mut video = Box::pin(DispiAdapter::run(vga.device.clone()));
    vga.io.write_mmio(TEXT_MEMORY_START, 2, 0x94db);
    vga.io.write_mmio(TEXT_MEMORY_START + 2, 2, 0x0720);
    crtc(&vga.io, 0x0f, 1);
    vga.io.write_port(CRTC_INDEX_PORT, 2, 0x060a);
    vga.io.write_port(CRTC_INDEX_PORT, 2, 0x070b);
    let source = vga.device.borrow().frames();
    tick(&clock, video.as_mut(), boundary(1));
    let on = source.try_recv().unwrap();
    assert!(on.vga().unwrap().cursor_blink_on);
    assert!(on.vga().unwrap().attribute_blink_on);
    let on_text = common::support::inspection::inspect(&on);
    assert_eq!(on_text.cursor.start_scan_line, 6);
    assert_eq!(on_text.cursor.end_scan_line, 7);
    let pixels = software::rasterize(&on).pixels.to_vec();
    let cursor = (6 * 640 + 8) * 4;
    assert_eq!(&pixels[..4], [170, 0, 0, 255]);
    assert_eq!(&pixels[cursor..cursor + 4], [170, 170, 170, 255]);
    for (frame, cursor_on, attribute_on) in [
        (8, false, true),
        (16, true, false),
        (24, false, false),
        (32, true, true),
        (1_000_024, false, false), // Late service uses all elapsed retraces.
    ] {
        tick(&clock, video.as_mut(), boundary(frame));
        let capture = source.try_recv().unwrap();

        assert_eq!(capture.vga().unwrap().cursor_blink_on, cursor_on);
        assert_eq!(capture.vga().unwrap().attribute_blink_on, attribute_on);
        assert_vga_memory_eq(&on, &capture);
        let image = software::rasterize(&capture);
        assert_eq!(
            &image.pixels[..4],
            if attribute_on {
                [170, 0, 0, 255]
            } else {
                [0, 0, 170, 255]
            }
        );
        assert_eq!(
            &image.pixels[cursor..cursor + 4],
            if cursor_on {
                [170, 170, 170, 255]
            } else {
                [0, 0, 0, 255]
            }
        );
    }
    drop(video);
    drop(vga);
    assert!(on.vga().unwrap().cursor_blink_on);
    assert!(on.vga().unwrap().attribute_blink_on);
    assert_eq!(&*software::rasterize(&on).pixels, pixels);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn screen_off_preserves_overdue_output_and_blanks_only_later_frames() {
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(common::text_vga(clock.clock()));
    let source = vga.device.borrow().frames();
    vga.io.write_mmio(TEXT_MEMORY_START, 2, 0x1edb);
    crtc(&vga.io, 0x0a, 0x20);
    clock.advance_to(clock.at(boundary(5) + Duration::from_micros(1)));
    vga.io.write_port(0x3c4, 2, 0x2101);
    vga.io.write_mmio(TEXT_MEMORY_START, 2, 0x2fdb);
    assert!(source.is_empty());
    common::service_video(video.as_mut());
    let before = source.try_recv().unwrap();

    assert_eq!(
        &software::rasterize(&before).pixels[..4],
        [255, 255, 85, 255]
    );
    clock.advance_to(clock.at(boundary(6)));
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    let blank = source.try_recv().unwrap();
    assert_ne!(blank.vga().unwrap().sequencer[1] & 0x20, 0);
    assert_eq!(before.vga().unwrap().sequencer[1] & 0x20, 0);
    assert!(
        software::rasterize(&blank)
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [0, 0, 0, 255])
    );
    vga.io.write_port(0x3c4, 2, 0x0101);
    clock.advance_to(clock.at(boundary(7)));
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    assert_eq!(
        &software::rasterize(&source.try_recv().unwrap()).pixels[..4],
        [255, 255, 255, 255]
    );
    assert_eq!(
        &software::rasterize(&before).pixels[..4],
        [255, 255, 85, 255]
    );
}
