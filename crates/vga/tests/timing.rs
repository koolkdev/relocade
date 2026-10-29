mod common;
use common::{boundary, crtc, dots, poll_video, ManualClock};
use relocade_machine::Device;
use relocade_vga::{Vga, TEXT_MEMORY_START};
use std::{
    cell::RefCell,
    pin::pin,
    rc::Rc,
    task::{Context, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn sequencer(vga: &mut impl Device, index: u8, value: u8) {
    vga.write_port(0x3c4, 2, u32::from(u16::from_le_bytes([index, value])));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn guest_status_tracks_display_enable_and_retrace_without_a_video_future() {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    vga.write_mmio(TEXT_MEMORY_START, 2, 0x0741);
    // Counts are physical dots: character height and double scanning do not
    // multiply the already programmed 449-line total.
    for (count, status) in [
        (0, 0),
        (639, 0),
        (640, 1),
        (672, 1),
        (800, 0),
        (399 * 800, 0),
        (400 * 800, 1),
        (412 * 800 - 1, 1),
        (412 * 800, 9),
        (414 * 800 - 1, 9),
        (414 * 800, 1),
        (449 * 800, 0),
    ] {
        clock.set(dots(count, 25_175_000));
        assert_eq!(vga.read_port(0x3da, 1), status, "dot {count}");
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
        let clock = Rc::new(ManualClock::default());
        let mut vga = Vga::with_clock(clock.clone());
        sequencer(&mut vga, 0, 1);
        vga.write_port(0x3c2, 1, misc);
        sequencer(&mut vga, 1, clocking);
        crtc(&mut vga, 0x11, 0x2e); // Unlock timing registers.
        crtc(&mut vga, 6, total);
        crtc(&mut vga, 7, overflow);
        crtc(&mut vga, 0x10, retrace);
        crtc(&mut vga, 0x12, display_end);
        sequencer(&mut vga, 0, 3);
        let (retrace_line, total_lines) = if total == 0x0b {
            (490, 525)
        } else {
            (412, 449)
        };
        let first = dots(retrace_line * 100 * dot_width, frequency);
        clock.set(first - Duration::from_nanos(1));
        assert_eq!(vga.read_port(0x3da, 1) & 8, 0);
        clock.set(first);
        assert_eq!(vga.read_port(0x3da, 1) & 8, 8);

        let second = dots((retrace_line + total_lines) * 100 * dot_width, frequency);
        clock.set(second - Duration::from_nanos(1));
        assert_eq!(vga.read_port(0x3da, 1) & 8, 0);
        clock.set(second);
        assert_eq!(vga.read_port(0x3da, 1) & 8, 8);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn overflow_retrace_end_and_vertical_counter_division_are_independent_of_character_height() {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    sequencer(&mut vga, 0, 1);
    crtc(&mut vga, 0x11, 0x21); // Retrace ends when the low four bits equal 1.
    crtc(&mut vga, 6, 0x56); // 600 vertical counts including blanking.
    crtc(&mut vga, 7, 0xe0); // Bit 9 of total, display end, and retrace start.
    crtc(&mut vga, 0x12, 7); // 520 displayed counts.
    crtc(&mut vga, 0x10, 0x0f); // Retrace at count 527; ends at 529.
    crtc(&mut vga, 0x17, 0xa7); // Each vertical count spans two horizontal lines.
    crtc(&mut vga, 9, 0x1f); // Character height is not a vertical timing divisor.
    sequencer(&mut vga, 0, 3);
    for (line, status) in [
        (519 * 2, 0),
        (520 * 2, 1),
        (527 * 2, 9),
        (529 * 2, 1),
        (600 * 2, 0),
    ] {
        clock.set(dots(line * 800, 25_175_000));
        assert_eq!(vga.read_port(0x3da, 1), status, "line {line}");
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn changing_the_vertical_clock_divider_preserves_the_live_counter() {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    clock.set(dots(412 * 800, 25_175_000));
    assert_eq!(vga.read_port(0x3da, 1), 9);
    crtc(&mut vga, 0x17, 0xa7);
    assert_eq!(vga.read_port(0x3da, 1), 9);
    // The existing counter is 412; reaching retrace end 414 now takes four
    // physical lines. The status follows the existing counter.
    clock.set(dots(415 * 800, 25_175_000));
    assert_eq!(vga.read_port(0x3da, 1), 9);
    clock.set(dots(416 * 800, 25_175_000));
    assert_eq!(vga.read_port(0x3da, 1), 1);
    crtc(&mut vga, 0x17, 0xa3);
    assert_eq!(vga.read_port(0x3da, 1), 1);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn changing_oscillators_uses_elapsed_old_clock_ticks_then_the_new_frequency() {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    clock.set(Duration::from_millis(1));
    vga.write_port(0x3c2, 1, 0x47);
    // One millisecond consumed exactly 25,175 dots. The remaining 304,425
    // dots to line 412 use 28.322 MHz, without resetting the raster phase.
    let deadline = Duration::from_millis(1) + dots(304_425, 28_322_000);
    clock.set(deadline - Duration::from_nanos(1));
    assert_eq!(vga.read_port(0x3da, 1) & 8, 0);
    clock.set(deadline);
    assert_eq!(vga.read_port(0x3da, 1) & 8, 8);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn clock_stop_preserves_phase_and_sequencer_reset_restarts_from_zero() {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    clock.set(dots(412 * 800, 25_175_000));
    assert_eq!(vga.read_port(0x3da, 1), 9);
    vga.write_port(0x3c2, 1, 0x4b);
    clock.set(Duration::from_secs(1));
    assert_eq!(vga.read_port(0x3da, 1), 9);
    vga.write_port(0x3c2, 1, 0x43);
    assert_eq!(vga.read_port(0x3da, 1), 9);
    clock.set(Duration::from_secs(1) + dots(2 * 800, 25_175_000));
    assert_eq!(vga.read_port(0x3da, 1), 1);

    // Sequencer reset discards that phase and holds the counters until released.
    sequencer(&mut vga, 0, 1);
    clock.set(Duration::from_secs(2));
    assert_eq!(vga.read_port(0x3da, 1), 1);
    sequencer(&mut vga, 0, 3);
    assert_eq!(vga.read_port(0x3da, 1), 0);
    clock.set(Duration::from_secs(2) + dots(412 * 800 - 1, 25_175_000));
    assert_eq!(vga.read_port(0x3da, 1), 1);
    clock.set(Duration::from_secs(2) + dots(412 * 800, 25_175_000));
    assert_eq!(vga.read_port(0x3da, 1), 9);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn deadlines_follow_the_device_origin_and_do_not_accumulate_rounding() {
    let clock = Rc::new(ManualClock::default());
    let origin = Duration::from_secs(12);
    clock.set(origin);
    let vga = Rc::new(RefCell::new(Vga::with_clock(clock.clone())));
    let mut video = pin!(Vga::run(vga.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(clock.deadline.get(), Some(origin + boundary(1)));
    for frame in [1, 2, 100, 1_000_000] {
        clock.set(origin + boundary(frame) - Duration::from_nanos(1));
        poll_video(video.as_mut(), &mut cx);
        assert_eq!(clock.deadline.get(), Some(origin + boundary(frame)));
        clock.set(origin + boundary(frame));
        poll_video(video.as_mut(), &mut cx);
        assert_eq!(clock.deadline.get(), Some(origin + boundary(frame + 1)));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn configuration_changes_replace_a_pending_deadline_and_restart_a_stopped_device() {
    let clock = Rc::new(ManualClock::default());
    let vga = Rc::new(RefCell::new(Vga::with_clock(clock.clone())));
    let mut video = pin!(Vga::run(vga.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(clock.deadline.get(), Some(boundary(1)));
    vga.borrow_mut().write_port(0x3c2, 1, 0x47);
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(clock.deadline.get(), Some(dots(412 * 800, 28_322_000)));
    sequencer(&mut *vga.borrow_mut(), 0, 1);
    poll_video(video.as_mut(), &mut cx);
    clock.set(Duration::from_secs(1));
    sequencer(&mut *vga.borrow_mut(), 0, 3);
    poll_video(video.as_mut(), &mut cx);
    assert_eq!(
        clock.deadline.get(),
        Some(Duration::from_secs(1) + dots(412 * 800, 28_322_000))
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn changing_character_width_preserves_the_horizontal_counter_and_partial_dot() {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    let changed_at = dots(79 * 8 + 4, 25_175_000);
    clock.set(changed_at);
    sequencer(&mut vga, 1, 0); // Still character 79, now nine dots wide.
    clock.set(changed_at + dots(4, 25_175_000));
    assert_eq!(vga.read_port(0x3da, 1), 0);
    clock.set(changed_at + dots(5, 25_175_000));
    assert_eq!(vga.read_port(0x3da, 1), 1);
}
