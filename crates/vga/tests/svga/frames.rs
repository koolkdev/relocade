use crate::common::{self, boundary, crtc, dots, poll_video, svga_boundary};
use common::support::{registers::palette, svga};
use relocade_machine::Device;
use relocade_machine::{ManualClock, Timestamp};
use relocade_vga::{Scanout, Vga};
use std::{
    cell::RefCell,
    future::Future,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Waker},
    time::Duration,
};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn packed_status_and_publication_ignore_legacy_timing_and_blink() {
    let clock = ManualClock::default();
    let mut vga = Vga::with_clock(clock.clock());
    svga::enable(&mut vga);
    let source = vga.frame_source();
    // Reset, disconnected clock, sync disable and custom CRTC totals remain
    // stored for VGA but must not change the fixed packed raster or its status.
    for (port, value) in [
        (0x3c4, 0x0100),
        (0x3c4, 0x2801),
        (0x3d4, 0x0011),
        (0x3d4, 0x0100),
        (0x3d4, 0x0007),
        (0x3d4, 0x0106),
        (0x3d4, 0x2317),
    ] {
        vga.write_port(port, 2, value);
    }
    vga.write_port(0x3c2, 1, 0x4b);
    crtc(&mut vga, 0x0d, 17); // The legacy start latch waits for a VGA retrace.
    for (count, status) in [
        (0, 0),
        (639, 0),
        (640, 1),
        (800, 0),
        (479 * 800, 0),
        (480 * 800, 1),
        (490 * 800 - 1, 1),
        (490 * 800, 9),
        (492 * 800 - 1, 9),
        (492 * 800, 1),
        (525 * 800, 0),
    ] {
        clock.advance_to(Timestamp::ZERO + dots(count, 25_175_000));
        assert_eq!(vga.read_port(0x3da, 1), status, "dot {count}");
    }
    let first = source.current_capture().unwrap();
    assert_eq!((first.sequence, first.time), (1, svga_boundary(1)));
    assert_eq!(first.display.geometry().size, [640, 480]);
    assert_eq!(vga.registers().crtc.display_start, 0);

    crtc(&mut vga, 0x0d, 19);
    svga::set(&mut vga, 5, 63);
    vga.write_port(0x3c8, 1, 200); // Transfer position alone changes no DAC color.
    clock.advance_to(Timestamp::ZERO + svga_boundary(24)); // Both VGA blink phases have changed.
    vga.read_port(0x3da, 1);
    assert!(Arc::ptr_eq(&first, &source.current_capture().unwrap()));

    palette(&mut vga, 1, [21, 42, 63]);
    clock.advance_to(Timestamp::ZERO + svga_boundary(25));
    vga.read_port(0x3da, 1);
    let recolored = source.current_capture().unwrap();
    assert_eq!(
        (recolored.sequence, recolored.time),
        (25, svga_boundary(25))
    );
    assert_eq!(recolored.display.dac.colors[1], [21, 42, 63]);
    assert_ne!(first.display.dac.colors[1], [21, 42, 63]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mode_switches_finish_old_boundaries_and_keep_sequences_and_retained_memory() {
    let clock = ManualClock::default();
    let mut vga = Vga::with_clock(clock.clock());
    let source = vga.frame_source();
    vga.write_mmio(0xb8000, 2, 0x0741);
    for (index, value) in [(1, 640), (2, 480), (3, 8)] {
        svga::set(&mut vga, index, value);
    }
    vga.write_port(0x1ce, 2, 4);
    let entered = boundary(3) + Duration::from_nanos(1);
    clock.advance_to(Timestamp::ZERO + entered);
    vga.write_port(0x1cf, 2, 1); // The actual enable services VGA before clearing VRAM.
    let legacy = source.current_capture().unwrap();
    assert_eq!((legacy.sequence, legacy.time), (3, boundary(3)));
    assert!(matches!(legacy.display.scanout(), Scanout::Vga(_)));
    assert_eq!(legacy.display.plane(0)[0], b'A');
    svga::write_byte(&mut vga, 307199, 6);

    clock.advance_to(Timestamp::ZERO + entered + svga_boundary(1) - Duration::from_nanos(1));
    assert_eq!(vga.read_port(0x3da, 1) & 8, 0);
    assert!(Arc::ptr_eq(&legacy, &source.current_capture().unwrap()));
    clock.advance_to(Timestamp::ZERO + entered + svga_boundary(1));
    vga.write_mmio(0xaafff, 1, 7); // Bank 4 is already selected; this write services the boundary.
    let packed = source.current_capture().unwrap();
    assert_eq!(
        (packed.sequence, packed.time),
        (4, entered + svga_boundary(1))
    );
    assert!(matches!(packed.display.scanout(), Scanout::Svga(_)));
    assert_eq!(packed.display.vram_byte(307199), Some(6));

    vga.write_port(0x1ce, 2, 4);
    let exited = entered + svga_boundary(2);
    clock.advance_to(Timestamp::ZERO + exited);
    vga.write_port(0x1cf, 2, 0); // The actual disable publishes packed state before switching.
    let last_packed = source.current_capture().unwrap();
    assert_eq!((last_packed.sequence, last_packed.time), (5, exited));
    assert_eq!(last_packed.display.vram_byte(307199), Some(7));
    assert!(matches!(last_packed.display.scanout(), Scanout::Svga(_)));
    vga.write_mmio(0xb8000, 2, 0x0742);
    clock.advance_to(Timestamp::ZERO + exited + boundary(1) - Duration::from_nanos(1));
    assert_eq!(vga.read_port(0x3da, 1) & 8, 0);
    assert!(Arc::ptr_eq(
        &last_packed,
        &source.current_capture().unwrap()
    ));
    clock.advance_to(Timestamp::ZERO + exited + boundary(1));
    vga.read_port(0x3da, 1);
    let resumed = source.current_capture().unwrap();
    assert_eq!((resumed.sequence, resumed.time), (6, exited + boundary(1)));
    assert!(matches!(resumed.display.scanout(), Scanout::Vga(_)));
    assert_eq!(resumed.display.plane(0)[0], b'B');
    drop(vga);
    assert_eq!(legacy.display.plane(0)[0], b'A');
    assert_eq!(packed.display.vram_byte(307199), Some(6));
    assert_eq!(last_packed.display.vram_byte(307199), Some(7));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mode_changes_wake_the_timer_and_slow_consumers_skip_unchanged_refreshes() {
    let clock = ManualClock::default();
    let origin = Duration::from_secs(5);
    clock.advance_to(Timestamp::ZERO + origin);
    let vga = Rc::new(RefCell::new(Vga::with_clock(clock.clock())));
    vga.borrow_mut().write_port(0x3c4, 2, 0x0100); // Hold the legacy raster in reset.
    let mut video = std::pin::pin!(Vga::run(vga.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock
            .next_deadline()
            .map(|at| at.duration_since(Timestamp::ZERO)),
        None
    );
    let source = vga.borrow().frame_source();
    // No-clear entry changes timing even though DISPI requests no memory effect.
    for (index, value) in [(1, 640), (2, 480), (3, 8), (4, 0x81)] {
        svga::set(&mut *vga.borrow_mut(), index, value);
    }
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock
            .next_deadline()
            .map(|at| at.duration_since(Timestamp::ZERO)),
        Some(origin + svga_boundary(1))
    );

    let mut first = std::pin::pin!(source.next_capture(None));
    assert!(first.as_mut().poll(&mut cx).is_pending());
    clock.advance_to(Timestamp::ZERO + origin + svga_boundary(1));
    poll_video(video.as_mut(), &mut cx);
    let Poll::Ready(Some(first)) = first.as_mut().poll(&mut cx) else {
        panic!("first packed frame")
    };
    assert_eq!(first.sequence, 1);
    svga::set(&mut *vga.borrow_mut(), 4, 1); // Repeated enable must not restart the phase.
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock
            .next_deadline()
            .map(|at| at.duration_since(Timestamp::ZERO)),
        Some(origin + svga_boundary(2))
    );
    let mut next = std::pin::pin!(source.next_capture(Some(first.sequence)));
    assert!(next.as_mut().poll(&mut cx).is_pending());
    clock.advance_to(Timestamp::ZERO + origin + svga_boundary(1_000_000));
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock
            .next_deadline()
            .map(|at| at.duration_since(Timestamp::ZERO)),
        Some(origin + svga_boundary(1_000_001))
    );
    assert!(next.as_mut().poll(&mut cx).is_pending());
    assert!(Arc::ptr_eq(&first, &source.current_capture().unwrap()));
    svga::write_byte(&mut *vga.borrow_mut(), 307199, 7);
    let exited = origin + svga_boundary(1_000_001);
    clock.advance_to(Timestamp::ZERO + exited);
    poll_video(video.as_mut(), &mut cx);
    let Poll::Ready(Some(latest)) = next.as_mut().poll(&mut cx) else {
        panic!("changed packed frame")
    };
    assert_eq!((latest.sequence, latest.time), (1_000_001, exited));
    assert_eq!(latest.display.vram_byte(307199), Some(7));

    svga::set(&mut *vga.borrow_mut(), 4, 0);
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock
            .next_deadline()
            .map(|at| at.duration_since(Timestamp::ZERO)),
        None
    ); // Return to the still-reset VGA raster.
    vga.borrow_mut().write_port(0x3c4, 2, 0x0300);
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock
            .next_deadline()
            .map(|at| at.duration_since(Timestamp::ZERO)),
        Some(exited + boundary(1))
    );
    clock.advance_to(Timestamp::ZERO + exited + boundary(1));
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(source.current_capture().unwrap().sequence, 1_000_002);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn indexed_modes_publish_at_their_own_dmt_retrace_boundaries() {
    for (width, height, clock_hz, line_dots, total_lines, retrace_start, retrace_end) in [
        (640, 480, 25_175_000, 800, 525, 490, 492),
        (800, 600, 40_000_000, 1056, 628, 601, 605),
        (1024, 768, 65_000_000, 1344, 806, 771, 777),
    ] {
        let clock = ManualClock::default();
        let mut vga = Vga::with_clock(clock.clock());
        common::support::svga::set_mode(&mut vga, width, height, 8);
        let start = common::dots(line_dots * retrace_start, clock_hz);
        clock.advance_to(Timestamp::ZERO + start - std::time::Duration::from_nanos(1));
        assert_eq!(vga.read_port(0x3da, 1) & 8, 0);
        assert!(vga.frame_source().current_capture().is_none());
        clock.advance_to(Timestamp::ZERO + start);
        assert_eq!(vga.read_port(0x3da, 1) & 8, 8);
        let capture = vga.frame_source().current_capture().unwrap();
        assert_eq!(capture.sequence, 1);
        assert_eq!(
            capture.display.geometry().size,
            [u32::from(width), u32::from(height)]
        );
        let end = common::dots(line_dots * retrace_end, clock_hz);
        clock.advance_to(Timestamp::ZERO + end);
        assert_eq!(vga.read_port(0x3da, 1) & 8, 0);
        vga.write_mmio(0xa0000, 1, 1);
        let second = common::dots(line_dots * (total_lines + retrace_start), clock_hz);
        clock.advance_to(Timestamp::ZERO + second - std::time::Duration::from_nanos(1));
        assert_eq!(vga.read_port(0x3da, 1) & 8, 0);
        assert_eq!(vga.frame_source().current_capture().unwrap().sequence, 1);
        clock.advance_to(Timestamp::ZERO + second);
        assert_eq!(vga.read_port(0x3da, 1) & 8, 8);
        let capture = vga.frame_source().current_capture().unwrap();
        assert_eq!(capture.sequence, 2);
        assert_eq!(capture.display.vram_byte(0), Some(1));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn pitch_and_origin_updates_preserve_raster_phase_and_completed_captures() {
    let clock = ManualClock::default();
    let mut vga = Vga::with_clock(clock.clock());
    svga::enable(&mut vga);
    clock.advance_to(Timestamp::ZERO + svga_boundary(1));
    vga.read_port(0x3da, 1);
    let original = vga.frame_source().current_capture().unwrap();
    clock.advance_to(Timestamp::ZERO + svga_boundary(2) - Duration::from_nanos(1));
    svga::set(&mut vga, 6, 1024);
    svga::set(&mut vga, 8, 32);
    svga::set(&mut vga, 9, 480);
    clock.advance_to(Timestamp::ZERO + svga_boundary(2));
    vga.read_port(0x3da, 1);
    let next = vga.frame_source().current_capture().unwrap();
    assert_eq!(next.sequence, 2);
    let Scanout::Svga(layout) = next.display.scanout() else {
        panic!("packed layout")
    };
    assert_eq!((layout.pitch(), layout.origin()), (1024, [32, 480]));
    let Scanout::Svga(old) = original.display.scanout() else {
        panic!("packed layout")
    };
    assert_eq!((old.pitch(), old.origin()), (640, [0, 0]));
}
