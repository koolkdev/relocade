mod common;
use common::{boundary, dots, poll_video, ManualClock};
use relocade_machine::Device;
use relocade_vga::Vga;
use std::{
    cell::RefCell,
    pin::pin,
    rc::Rc,
    task::{Context, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn status_reports_fixed_display_and_retrace_without_an_async_poll() {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
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
fn timing_registers_keep_readback_while_the_initial_raster_stays_fixed() {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    vga.write_port(0x3c2, 1, 0x46); // Select monochrome ports and another clock.
    vga.write_port(0x3b4, 2, 0x0011);
    vga.write_port(0x3b4, 2, 0x1000);
    assert_eq!(vga.read_port(0x3b5, 1), 0x10);
    clock.set(dots(412 * 800, 25_175_000));
    assert_eq!(vga.read_port(0x3ba, 1), 9);
    assert_eq!(vga.read_port(0x3da, 1), 0xff);
}
