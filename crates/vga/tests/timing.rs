use crate::common::fixture::ConnectedVga;
use relocade_clock_test_support::ManualClock;
mod common;
use common::{boundary, crtc, dots, poll_video};
use relocade_machine::Device;
use relocade_vga::DispiAdapter;
use std::{
    cell::RefCell,
    pin::pin,
    rc::Rc,
    task::{Context, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn sequencer(io: &relocade_machine::IoBus, index: u8, value: u8) {
    io.write_port(0x3c4, 2, u32::from(u16::from_le_bytes([index, value])));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn blink_dividers_follow_retraces_at_different_dot_clocks() {
    for (miscellaneous, frequency) in [(0x43, 25_175_000), (0x47, 28_322_000)] {
        let clock = ManualClock::default();
        let (vga, mut video) = common::start_video(ConnectedVga::new(clock.clock()));
        vga.io.write_port(0x3c2, 1, miscellaneous);
        let source = vga.device.borrow().frames();
        let mut latest = None;
        // Cursor and attribute phases differ over each half of a 32-frame cycle.
        for (frame, changed, cursor_on, attribute_on) in [
            (1, true, true, true),
            (7, false, true, true),
            (8, true, false, true),
            (15, false, false, true),
            (16, true, true, false),
            (23, false, true, false),
            (24, true, false, false),
            (31, false, false, false),
            (32, true, true, true),
        ] {
            clock.advance_to(clock.at(dots((412 + (frame - 1) * 449) * 800, frequency)));
            vga.io.read_port(0x3da, 1);
            common::service_video(video.as_mut());
            if changed {
                latest = Some(source.try_recv().unwrap());
            } else {
                assert!(source.is_empty(), "unchanged inputs at frame {frame}");
            }
            let capture = latest.as_ref().unwrap();
            assert_eq!(
                capture.vga().unwrap().cursor_blink_on,
                cursor_on,
                "frame {frame}"
            );
            assert_eq!(capture.vga().unwrap().attribute_blink_on, attribute_on);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn guest_status_tracks_display_enable_and_retrace_without_a_video_future() {
    let clock = ManualClock::default();
    let vga = ConnectedVga::new(clock.clock());
    // CRTC blanking intersects active display but does not change the separate
    // display-enable signal reported by Input Status 1 bit zero.
    crtc(&vga.io, 0x11, 0x2e);
    crtc(&vga.io, 2, 0);
    crtc(&vga.io, 3, 0x82);
    crtc(&vga.io, 0x15, 0);
    crtc(&vga.io, 0x16, 2);
    // Counts are physical dots: character height and double scanning do not
    // multiply the already programmed 449-line total.
    for (count, status) in [
        (0, 0),
        (8, 0), // Inside programmed horizontal blanking, with display enabled.
        (639, 0),
        (640, 1),
        (672, 1),
        (800, 0),
        (257 * 800, 0), // Inside programmed vertical blanking.
        (399 * 800, 0),
        (400 * 800, 1),
        (412 * 800 - 1, 1),
        (412 * 800, 9),
        (414 * 800 - 1, 9),
        (414 * 800, 1),
        (449 * 800, 0),
    ] {
        clock.advance_to(clock.at(dots(count, 25_175_000)));
        assert_eq!(vga.io.read_port(0x3da, 1), status, "dot {count}");
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn programmed_totals_and_clock_selection_determine_frame_boundaries() {
    for (misc, clocking, total, overflow, retrace, display_end, dot_width, frequency) in [
        // 640x480 timing: 25.175 MHz / (800 * 525) = 59.94 Hz.
        (0xc3, 1, 0x0b, 0x3e, 0xea, 0xdf, 8, 25_175_000),
        // 720x400 text timing: 28.322 MHz / (900 * 449) = 70.09 Hz.
        (0x47, 0, 0xbf, 0x1f, 0x9c, 0x8f, 9, 28_322_000),
        // Dividing the dot clock doubles every raster interval.
        (0x47, 8, 0xbf, 0x1f, 0x9c, 0x8f, 18, 28_322_000),
    ] {
        let clock = ManualClock::default();
        let (vga, mut video) = common::start_video(ConnectedVga::new(clock.clock()));
        sequencer(&vga.io, 0, 1);
        vga.io.write_port(0x3c2, 1, misc);
        sequencer(&vga.io, 1, clocking);
        crtc(&vga.io, 0x11, 0x2e); // Unlock timing registers.
        crtc(&vga.io, 6, total);
        crtc(&vga.io, 7, overflow);
        crtc(&vga.io, 0x10, retrace);
        crtc(&vga.io, 0x12, display_end);
        sequencer(&vga.io, 0, 3);
        let source = vga.device.borrow().frames();
        let (retrace_line, total_lines) = if total == 0x0b {
            (490, 525)
        } else {
            (412, 449)
        };
        let first = dots(retrace_line * 100 * dot_width, frequency);
        clock.advance_to(clock.at(first - Duration::from_nanos(1)));
        assert_eq!(vga.io.read_port(0x3da, 1) & 8, 0);
        common::service_video(video.as_mut());
        assert!(source.is_empty());
        clock.advance_to(clock.at(first));
        assert_eq!(vga.io.read_port(0x3da, 1) & 8, 8);
        common::service_video(video.as_mut());
        assert!(source.try_recv().is_ok());
        let second = dots((retrace_line + total_lines) * 100 * dot_width, frequency);
        clock.advance_to(clock.at(second - Duration::from_nanos(1)));
        assert_eq!(vga.io.read_port(0x3da, 1) & 8, 0);
        common::service_video(video.as_mut());
        clock.advance_to(clock.at(second));
        assert_eq!(vga.io.read_port(0x3da, 1) & 8, 8);
        common::service_video(video.as_mut());
        // Retrace still advances without replacing unchanged display inputs.
        assert!(source.is_empty());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn overflow_retrace_end_and_vertical_counter_division_are_independent_of_character_height() {
    let clock = ManualClock::default();
    let vga = ConnectedVga::new(clock.clock());
    sequencer(&vga.io, 0, 1);
    crtc(&vga.io, 0x11, 0x21); // Retrace ends when the low four bits equal 1.
    crtc(&vga.io, 6, 0x56); // 600 vertical counts including blanking.
    crtc(&vga.io, 7, 0xe0); // Bit 9 of total, display end, and retrace start.
    crtc(&vga.io, 0x12, 7); // 520 displayed counts.
    crtc(&vga.io, 0x10, 0x0f); // Retrace at count 527; ends at 529.
    crtc(&vga.io, 0x17, 0xa7); // Each vertical count spans two horizontal lines.
    crtc(&vga.io, 9, 0x1f); // Character height is not a vertical timing divisor.
    sequencer(&vga.io, 0, 3);
    for (line, status) in [
        (519 * 2, 0),
        (520 * 2, 1),
        (527 * 2, 9),
        (529 * 2, 1),
        (600 * 2, 0),
    ] {
        clock.advance_to(clock.at(dots(line * 800, 25_175_000)));
        assert_eq!(vga.io.read_port(0x3da, 1), status, "line {line}");
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn changing_the_vertical_clock_divider_preserves_the_live_counter() {
    let clock = ManualClock::default();
    let vga = ConnectedVga::new(clock.clock());
    clock.advance_to(clock.at(dots(412 * 800, 25_175_000)));
    assert_eq!(vga.io.read_port(0x3da, 1), 9);
    crtc(&vga.io, 0x17, 0xa7);
    assert_eq!(vga.io.read_port(0x3da, 1), 9);
    // The existing counter is 412; reaching retrace end 414 now takes four
    // physical lines. The status follows the existing counter.
    clock.advance_to(clock.at(dots(415 * 800, 25_175_000)));
    assert_eq!(vga.io.read_port(0x3da, 1), 9);
    clock.advance_to(clock.at(dots(416 * 800, 25_175_000)));
    assert_eq!(vga.io.read_port(0x3da, 1), 1);
    crtc(&vga.io, 0x17, 0xa3);
    assert_eq!(vga.io.read_port(0x3da, 1), 1);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn clearing_the_vertical_clock_divider_preserves_the_second_scanlines_position() {
    let clock = ManualClock::default();
    let vga = ConnectedVga::new(clock.clock());
    crtc(&vga.io, 0x17, 0xa7);
    // Count 411, dot 100 of its second physical scanline.
    let changed_at = 823 * 800 + 100;
    clock.advance_to(clock.at(dots(changed_at, 25_175_000)));
    crtc(&vga.io, 0x17, 0xa3);
    assert_eq!(vga.io.read_port(0x3da, 1) & 8, 0);
    // Count 412 starts after the remaining 700 dots, not on the next dot.
    clock.advance_to(clock.at(dots(changed_at + 699, 25_175_000)));
    assert_eq!(vga.io.read_port(0x3da, 1) & 8, 0);
    clock.advance_to(clock.at(dots(changed_at + 700, 25_175_000)));
    assert_eq!(vga.io.read_port(0x3da, 1) & 8, 8);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn retrace_end_comparison_continues_after_the_vertical_counter_wraps() {
    let clock = ManualClock::default();
    let vga = ConnectedVga::new(clock.clock());
    sequencer(&vga.io, 0, 1);
    for (index, value) in [(0x11, 1), (6, 18), (7, 0), (0x10, 17)] {
        crtc(&vga.io, index, value);
    }
    sequencer(&vga.io, 0, 3);
    for (line, retrace) in [(16, 0), (17, 8), (19, 8), (20, 8), (21, 0)] {
        clock.advance_to(clock.at(dots(line * 800, 25_175_000)));
        assert_eq!(vga.io.read_port(0x3da, 1) & 8, retrace, "line {line}");
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn disabling_sync_suppresses_the_pulse_without_resetting_counters_or_capture_boundary() {
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(ConnectedVga::new(clock.clock()));
    let source = vga.device.borrow().frames();
    clock.advance_to(clock.at(dots(411 * 800, 25_175_000)));
    crtc(&vga.io, 0x17, 0x23);
    clock.advance_to(clock.at(boundary(1)));
    assert_eq!(vga.io.read_port(0x3da, 1), 1);
    common::service_video(video.as_mut());
    let capture = source.try_recv().unwrap();
    assert_eq!(capture.vga().unwrap().crtc.values[0x17], 0x23);
    crtc(&vga.io, 0x17, 0xa3);
    assert_eq!(vga.io.read_port(0x3da, 1), 9);
    common::service_video(video.as_mut());
    clock.advance_to(clock.at(dots(414 * 800, 25_175_000)));
    assert_eq!(vga.io.read_port(0x3da, 1), 1);
    common::service_video(video.as_mut());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn changing_oscillators_uses_elapsed_old_clock_ticks_then_the_new_frequency() {
    let clock = ManualClock::default();
    let vga = ConnectedVga::new(clock.clock());
    clock.advance_to(clock.at(Duration::from_millis(1)));
    vga.io.write_port(0x3c2, 1, 0x47);
    // One millisecond consumed exactly 25,175 dots. The remaining 304,425
    // dots to line 412 use 28.322 MHz, without resetting the raster phase.
    let deadline = Duration::from_millis(1) + dots(304_425, 28_322_000);
    clock.advance_to(clock.at(deadline - Duration::from_nanos(1)));
    assert_eq!(vga.io.read_port(0x3da, 1) & 8, 0);
    clock.advance_to(clock.at(deadline));
    assert_eq!(vga.io.read_port(0x3da, 1) & 8, 8);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn clock_stop_preserves_phase_and_sequencer_reset_restarts_from_zero() {
    let clock = ManualClock::default();
    let vga = ConnectedVga::new(clock.clock());
    clock.advance_to(clock.at(dots(412 * 800, 25_175_000)));
    assert_eq!(vga.io.read_port(0x3da, 1), 9);
    vga.io.write_port(0x3c2, 1, 0x4b);
    clock.advance_to(clock.at(Duration::from_secs(1)));
    assert_eq!(vga.io.read_port(0x3da, 1), 9);
    vga.io.write_port(0x3c2, 1, 0x43);
    assert_eq!(vga.io.read_port(0x3da, 1), 9);
    clock.advance_to(clock.at(Duration::from_secs(1) + dots(2 * 800, 25_175_000)));
    assert_eq!(vga.io.read_port(0x3da, 1), 1);

    // Sequencer reset discards that phase and holds the counters until released.
    sequencer(&vga.io, 0, 1);
    clock.advance_to(clock.at(Duration::from_secs(2)));
    assert_eq!(vga.io.read_port(0x3da, 1), 1);
    sequencer(&vga.io, 0, 3);
    assert_eq!(vga.io.read_port(0x3da, 1), 0);
    clock.advance_to(clock.at(Duration::from_secs(2) + dots(412 * 800 - 1, 25_175_000)));
    assert_eq!(vga.io.read_port(0x3da, 1), 1);
    clock.advance_to(clock.at(Duration::from_secs(2) + dots(412 * 800, 25_175_000)));
    assert_eq!(vga.io.read_port(0x3da, 1), 9);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn deadlines_follow_the_device_origin_and_do_not_accumulate_rounding() {
    let clock = ManualClock::default();
    let origin = Duration::from_secs(12);
    clock.advance_to(clock.at(origin));
    let vga = Rc::new(RefCell::new(DispiAdapter::with_clock(clock.clock())));
    let mut video = pin!(DispiAdapter::run(vga.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock.next_deadline().map(|at| clock.elapsed(at)),
        Some(origin + boundary(1))
    );
    for frame in [1, 2, 100, 1_000_000] {
        clock.advance_to(clock.at(origin + boundary(frame) - Duration::from_nanos(1)));
        poll_video(video.as_mut(), &mut cx);
        assert_eq!(
            clock.next_deadline().map(|at| clock.elapsed(at)),
            Some(origin + boundary(frame))
        );
        clock.advance_to(clock.at(origin + boundary(frame)));
        poll_video(video.as_mut(), &mut cx);
        assert_eq!(
            clock.next_deadline().map(|at| clock.elapsed(at)),
            Some(origin + boundary(frame + 1))
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn configuration_changes_replace_a_pending_deadline_and_restart_a_stopped_device() {
    let clock = ManualClock::default();
    let vga = ConnectedVga::new(clock.clock());
    let mut video = pin!(DispiAdapter::run(vga.device.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock.next_deadline().map(|at| clock.elapsed(at)),
        Some(boundary(1))
    );
    vga.io.write_port(0x3c2, 1, 0x47);
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock.next_deadline().map(|at| clock.elapsed(at)),
        Some(dots(412 * 800, 28_322_000))
    );
    sequencer(&vga.io, 0, 1);
    poll_video(video.as_mut(), &mut cx);
    clock.advance_to(clock.at(Duration::from_secs(1)));
    sequencer(&vga.io, 0, 3);
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock.next_deadline().map(|at| clock.elapsed(at)),
        Some(Duration::from_secs(1) + dots(412 * 800, 28_322_000))
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn changing_character_width_preserves_the_horizontal_counter_and_partial_dot() {
    let clock = ManualClock::default();
    let vga = ConnectedVga::new(clock.clock());
    let changed_at = dots(79 * 8 + 4, 25_175_000);
    clock.advance_to(clock.at(changed_at));
    sequencer(&vga.io, 1, 0); // Still character 79, now nine dots wide.
    clock.advance_to(clock.at(changed_at + dots(4, 25_175_000)));
    assert_eq!(vga.io.read_port(0x3da, 1), 0);
    clock.advance_to(clock.at(changed_at + dots(5, 25_175_000)));
    assert_eq!(vga.io.read_port(0x3da, 1), 1);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn native_guest_deadlines_reach_status_edges_through_the_connected_bus() {
    use relocade_machine::{GuestTimeDriver, IoBus, Timestamp};
    use std::{future::Future, num::NonZeroU64};
    let driver = GuestTimeDriver::new(NonZeroU64::new(25_175_000).unwrap());
    let clock = driver.clock();
    let start = clock.now();
    let vga = Rc::new(RefCell::new(DispiAdapter::with_clock(clock.clone())));
    let bus = IoBus::new(vga.clone(), |io| DispiAdapter::configure_io(io, &vga));
    for (count, status) in [(639, 0), (640, 1), (800, 0), (412 * 800, 9), (414 * 800, 1)] {
        let deadline = clock.add(start, dots(count, 25_175_000));
        // Service a cold edge one native tick late. The following cached edge
        // must still occur on the raster's nanosecond grid.
        let fraction = u128::from(count == 640 || count == 412 * 800);
        let mut wait = pin!(clock.sleep_until(Timestamp::from_ticks(deadline.ticks() + fraction)));
        assert!(wait
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        assert!(driver.advance_to_next_deadline());
        assert_eq!(bus.read_port(0x3da, 1), status, "dot {count}");
    }
}
