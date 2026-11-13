use crate::common::fixture::ConnectedVga;
use crate::common::{self, boundary, crtc, dots, poll_video, svga_boundary};
use common::support::{registers::palette, svga};
use relocade_clock_test_support::ManualClock;
use relocade_machine::Device;
use relocade_vga::{DispiAdapter, Scanout};
use std::{
    future::Future,
    task::{Context, Poll, Waker},
    time::Duration,
};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn packed_status_and_publication_ignore_legacy_timing_and_blink() {
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(ConnectedVga::new(clock.clock()));
    svga::enable(&vga.io);
    let source = vga.device.borrow().frames();
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
        vga.io.write_port(port, 2, value);
    }
    vga.io.write_port(0x3c2, 1, 0x4b);
    crtc(&vga.io, 0x0d, 17); // The legacy start latch waits for a VGA retrace.
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
        clock.advance_to(clock.at(dots(count, 25_175_000)));
        assert_eq!(vga.io.read_port(0x3da, 1), status, "dot {count}");
        common::service_video(video.as_mut());
    }
    let first = source.try_recv().unwrap();

    assert_eq!(first.geometry().size, [640, 480]);
    assert_eq!(vga.device.borrow().registers().crtc.display_start, 0);

    crtc(&vga.io, 0x0d, 19);
    svga::set(&vga.io, 5, 63);
    vga.io.write_port(0x3c8, 1, 200); // Transfer position alone changes no DAC color.
    clock.advance_to(clock.at(svga_boundary(24))); // Both VGA blink phases have changed.
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    assert!(source.is_empty());

    palette(&vga.io, 1, [21, 42, 63]);
    clock.advance_to(clock.at(svga_boundary(25)));
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    let recolored = source.try_recv().unwrap();

    assert_eq!(recolored.dac.colors[1], [21, 42, 63]);
    assert_ne!(first.dac.colors[1], [21, 42, 63]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mode_switches_finish_old_boundaries_and_preserve_retained_memory() {
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(ConnectedVga::new(clock.clock()));
    let source = vga.device.borrow().frames();
    vga.io.write_mmio(0xb8000, 2, 0x0741);
    for (index, value) in [(1, 640), (2, 480), (3, 8)] {
        svga::set(&vga.io, index, value);
    }
    vga.io.write_port(0x1ce, 2, 4);
    let entered = boundary(3) + Duration::from_nanos(1);
    clock.advance_to(clock.at(entered));
    vga.io.write_port(0x1cf, 2, 1); // The actual enable services VGA before clearing VRAM.
    assert!(source.is_empty());
    common::service_video(video.as_mut());
    let legacy = source.try_recv().unwrap();

    assert!(matches!(legacy.scanout(), Scanout::Vga(_)));
    assert_eq!(legacy.plane_byte(0, 0), b'A');
    svga::write_byte(&vga.io, 307199, 6);

    clock.advance_to(clock.at(entered + svga_boundary(1) - Duration::from_nanos(1)));
    assert_eq!(vga.io.read_port(0x3da, 1) & 8, 0);
    common::service_video(video.as_mut());
    assert!(source.is_empty());
    clock.advance_to(clock.at(entered + svga_boundary(1)));
    vga.io.write_mmio(0xaafff, 1, 7); // Bank 4 is already selected; this write services the boundary.
    common::service_video(video.as_mut());
    let packed = source.try_recv().unwrap();

    assert!(matches!(packed.scanout(), Scanout::Packed(_)));
    assert_eq!(packed.vram_byte(307199), Some(6));

    vga.io.write_port(0x1ce, 2, 4);
    let exited = entered + svga_boundary(2);
    clock.advance_to(clock.at(exited));
    vga.io.write_port(0x1cf, 2, 0); // Capture packed state before disabling it.
    common::service_video(video.as_mut());
    let last_packed = source.try_recv().unwrap();

    assert_eq!(last_packed.vram_byte(307199), Some(7));
    assert!(matches!(last_packed.scanout(), Scanout::Packed(_)));
    vga.io.write_mmio(0xb8000, 2, 0x0742);
    clock.advance_to(clock.at(exited + boundary(1) - Duration::from_nanos(1)));
    assert_eq!(vga.io.read_port(0x3da, 1) & 8, 0);
    common::service_video(video.as_mut());
    assert!(source.is_empty());
    clock.advance_to(clock.at(exited + boundary(1)));
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    let resumed = source.try_recv().unwrap();

    assert!(matches!(resumed.scanout(), Scanout::Vga(_)));
    assert_eq!(resumed.plane_byte(0, 0), b'B');
    drop(video);
    drop(vga);
    assert_eq!(legacy.plane_byte(0, 0), b'A');
    assert_eq!(packed.vram_byte(307199), Some(6));
    assert_eq!(last_packed.vram_byte(307199), Some(7));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mode_changes_wake_the_timer_and_slow_consumers_skip_unchanged_refreshes() {
    let clock = ManualClock::default();
    let origin = Duration::from_secs(5);
    clock.advance_to(clock.at(origin));
    let vga = ConnectedVga::new(clock.clock());
    vga.io.write_port(0x3c4, 2, 0x0100); // Hold the legacy raster in reset.
    let mut video = std::pin::pin!(DispiAdapter::run(vga.device.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(clock.next_deadline().map(|at| clock.elapsed(at)), None);
    let source = vga.device.borrow().frames();
    // No-clear entry changes timing even though DISPI requests no memory effect.
    for (index, value) in [(1, 640), (2, 480), (3, 8), (4, 0x81)] {
        svga::set(&vga.io, index, value);
    }
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock.next_deadline().map(|at| clock.elapsed(at)),
        Some(origin + svga_boundary(1))
    );

    let mut first = std::pin::pin!(source.recv());
    assert!(first.as_mut().poll(&mut cx).is_pending());
    clock.advance_to(clock.at(origin + svga_boundary(1)));
    poll_video(video.as_mut(), &mut cx);
    let Poll::Ready(Ok(first)) = first.as_mut().poll(&mut cx) else {
        panic!("first packed frame")
    };
    assert!(matches!(first.scanout(), Scanout::Packed(_)));

    svga::set(&vga.io, 4, 1); // Repeated enable must not restart the phase.
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock.next_deadline().map(|at| clock.elapsed(at)),
        Some(origin + svga_boundary(2))
    );
    let mut next = std::pin::pin!(source.recv());
    assert!(next.as_mut().poll(&mut cx).is_pending());
    clock.advance_to(clock.at(origin + svga_boundary(1_000_000)));
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock.next_deadline().map(|at| clock.elapsed(at)),
        Some(origin + svga_boundary(1_000_001))
    );
    assert!(next.as_mut().poll(&mut cx).is_pending());
    assert!(source.is_empty());
    svga::write_byte(&vga.io, 307199, 7);
    let exited = origin + svga_boundary(1_000_001);
    clock.advance_to(clock.at(exited));
    poll_video(video.as_mut(), &mut cx);
    let Poll::Ready(Ok(latest)) = next.as_mut().poll(&mut cx) else {
        panic!("changed packed frame")
    };

    assert_eq!(latest.vram_byte(307199), Some(7));

    svga::set(&vga.io, 4, 0);
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(clock.next_deadline().map(|at| clock.elapsed(at)), None); // Return to the still-reset VGA raster.
    vga.io.write_port(0x3c4, 2, 0x0300);
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock.next_deadline().map(|at| clock.elapsed(at)),
        Some(exited + boundary(1))
    );
    clock.advance_to(clock.at(exited + boundary(1)));
    poll_video(video.as_mut(), &mut cx);
    assert!(matches!(
        source.try_recv().unwrap().scanout(),
        Scanout::Vga(_)
    ));
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
        let (vga, mut video) = common::start_video(ConnectedVga::new(clock.clock()));
        common::support::svga::set_mode(&vga.io, width, height, 8);
        let start = common::dots(line_dots * retrace_start, clock_hz);
        clock.advance_to(clock.at(start - std::time::Duration::from_nanos(1)));
        assert_eq!(vga.io.read_port(0x3da, 1) & 8, 0);
        common::service_video(video.as_mut());
        assert!(vga.device.borrow().frames().is_empty());
        clock.advance_to(clock.at(start));
        assert_eq!(vga.io.read_port(0x3da, 1) & 8, 8);
        common::service_video(video.as_mut());
        let capture = vga.device.borrow().frames().try_recv().unwrap();

        assert_eq!(
            capture.geometry().size,
            [u32::from(width), u32::from(height)]
        );
        let end = common::dots(line_dots * retrace_end, clock_hz);
        clock.advance_to(clock.at(end));
        assert_eq!(vga.io.read_port(0x3da, 1) & 8, 0);
        common::service_video(video.as_mut());
        vga.io.write_mmio(0xa0000, 1, 1);
        let second = common::dots(line_dots * (total_lines + retrace_start), clock_hz);
        clock.advance_to(clock.at(second - std::time::Duration::from_nanos(1)));
        assert_eq!(vga.io.read_port(0x3da, 1) & 8, 0);
        common::service_video(video.as_mut());
        assert!(vga.device.borrow().frames().is_empty());
        clock.advance_to(clock.at(second));
        assert_eq!(vga.io.read_port(0x3da, 1) & 8, 8);
        common::service_video(video.as_mut());
        let capture = vga.device.borrow().frames().try_recv().unwrap();

        assert_eq!(capture.vram_byte(0), Some(1));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn pitch_and_origin_updates_preserve_raster_phase_and_completed_captures() {
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(ConnectedVga::new(clock.clock()));
    svga::enable(&vga.io);
    clock.advance_to(clock.at(svga_boundary(1)));
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    let original = vga.device.borrow().frames().try_recv().unwrap();
    clock.advance_to(clock.at(svga_boundary(2) - Duration::from_nanos(1)));
    svga::set(&vga.io, 6, 1024);
    svga::set(&vga.io, 8, 32);
    svga::set(&vga.io, 9, 480);
    clock.advance_to(clock.at(svga_boundary(2)));
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    let next = vga.device.borrow().frames().try_recv().unwrap();

    let Scanout::Packed(layout) = next.scanout() else {
        panic!("packed layout")
    };
    assert_eq!((layout.pitch(), layout.origin()), (1024, [32, 480]));
    let Scanout::Packed(old) = original.scanout() else {
        panic!("packed layout")
    };
    assert_eq!((old.pitch(), old.origin()), (640, [0, 0]));
}
