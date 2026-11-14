//! A second card exercises the shared VGA wiring through the real bus.
use relocade_clock_test_support::ManualClock;
use relocade_machine::{Device, IoBus};
use relocade_vga::{Et4000Ax, FramebufferAccess, RetraceEvent};
use std::{
    cell::RefCell,
    future::Future,
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(feature = "renderer")]
#[path = "et4000/gpu.rs"]
mod gpu;
#[path = "et4000/memory.rs"]
mod memory;
#[path = "et4000/planar.rs"]
mod planar;
#[path = "et4000/ramdac.rs"]
mod ramdac;
#[cfg(feature = "renderer")]
#[path = "gpu/readback.rs"]
mod readback;
#[path = "et4000/registers.rs"]
mod registers;
#[path = "et4000/scanout.rs"]
mod scanout;
#[path = "et4000/state.rs"]
mod state;
mod support;
#[path = "et4000/timing.rs"]
mod timing;

struct ConnectedEt4000 {
    card: Rc<RefCell<Et4000Ax>>,
    io: IoBus,
}

impl ConnectedEt4000 {
    fn new(clock: &ManualClock) -> Self {
        let card = Rc::new(RefCell::new(Et4000Ax::with_clock(clock.clock())));
        let io = IoBus::new(card.clone(), |io| Et4000Ax::configure_io(io, &card));
        Self { card, io }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_ports_program_the_core_without_a_dispi_interface() {
    let device = ConnectedEt4000::new(&ManualClock::default());
    let io = &device.io;
    assert_eq!(device.card.borrow().video_memory_bytes(), 1024 * 1024);
    assert_eq!(io.read_port(0x1ce, 2), 0xffff);
    assert_eq!(io.read_port(0x1cf, 2), 0xffff);
    io.write_mmio(0xb8000, 2, 0x0741);
    assert_eq!(io.read_mmio(0xb8000, 2), 0x0741);

    // Word writes select a live register before the following data byte.
    for (port, value) in [
        (0x3c4, 0x0604), // Planar CPU addressing.
        (0x3ce, 0x0005),
        (0x3ce, 0x0506), // A0000 aperture.
        (0x3c4, 0x0102), // Plane zero only.
    ] {
        io.write_port(port, 2, value);
    }
    io.write_mmio(0xa0000, 1, 0x5a);
    io.write_port(0x3c4, 2, 0x0402); // Plane two only.
    io.write_mmio(0xa0000, 1, 0xa5);
    io.write_port(0x3ce, 2, 0x0004);
    assert_eq!(io.read_mmio(0xa0000, 1), 0x5a);
    io.write_port(0x3ce, 2, 0x0204);
    assert_eq!(io.read_mmio(0xa0000, 1), 0xa5);

    io.write_port(0x3c8, 1, 9);
    for component in [12, 23, 34] {
        io.write_port(0x3c9, 1, component);
    }
    assert_eq!(device.card.borrow().snapshot().dac.colors[9], [12, 23, 34]);

    io.write_port(0x3d4, 2, 0x120c);
    io.write_port(0x3d4, 2, 0xabec); // Bits 6–7 reserved; 2Ch must not alias 0Ch.
    assert_eq!(io.read_port(0x3d4, 2), 0xff2c);
    io.write_port(0x3d4, 1, 0x0c);
    assert_eq!(io.read_port(0x3d5, 1), 0x12);

    io.write_port(0x3c2, 1, 0x42); // Monochrome address group, same CRTC state.
    assert_eq!(io.read_port(0x3d4, 2), 0xffff);
    assert_eq!(io.read_port(0x3b4, 2), 0x120c);
    io.write_port(0x46e8, 1, 0);
    assert_eq!(io.read_port(0x3b4, 2), 0xffff);
    assert_eq!(io.read_mmio(0xa0000, 1), 0xff);
    io.write_mmio(0xa0000, 1, 0);
    io.write_port(0x46e8, 1, 0x0e);
    assert_eq!(io.read_port(0x3b4, 2), 0x120c);
    assert_eq!(io.read_mmio(0xa0000, 1), 0xa5);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cached_status_resets_attribute_phase_without_borrowing_the_card() {
    let device = ConnectedEt4000::new(&ManualClock::default());
    device.io.write_port(0x3c0, 1, 0x01); // Awaiting palette data.
    {
        let _borrow = device.card.borrow_mut();
        assert_eq!(device.io.read_port(0x3da, 2), 0xff80);
    }
    device.io.write_port(0x3c0, 1, 0x02); // Status read restored the index phase.
    device.io.write_port(0x3c0, 1, 0x35);
    assert_eq!(device.io.read_port(0x3c1, 1), 0x35);

    let mut wait = pin!(device.card.borrow_mut().wait_for_retrace().unwrap());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    device.io.write_port(0x3c2, 1, 0x42);
    assert_eq!(
        wait.as_mut().poll(&mut cx),
        Poll::Ready(RetraceEvent::Reconfigured)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn display_task_latches_the_viewport_and_keeps_retained_frames_immutable() {
    let clock = ManualClock::default();
    let device = ConnectedEt4000::new(&clock);
    let frames = device.card.borrow().frames();
    let mut task = pin!(Et4000Ax::run(device.card.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    device.io.write_mmio(0xb8000, 4, 0x0742_0741);
    device.io.write_port(0x3d4, 2, 0x010d);

    let before = device.card.borrow().core_state();
    let cell = device
        .card
        .borrow_mut()
        .framebuffer()
        .text_cell(0, 0)
        .unwrap();
    assert_eq!(
        cell.value(),
        0x0742,
        "native drawing uses the programmed page"
    );
    device
        .card
        .borrow_mut()
        .framebuffer()
        .write_text_cell(&cell, 0x1e43);
    assert_eq!(device.card.borrow().core_state(), before);
    assert_eq!(
        device.card.borrow().snapshot().vga().unwrap().display_start,
        0
    );

    clock.advance_to(clock.at(Duration::from_millis(20)));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let first = frames.try_recv().unwrap();
    assert_eq!(first.vga().unwrap().display_start, 1);
    assert_eq!(first.vram_byte(0), Some(b'A'));
    assert_eq!(first.vram_byte(8), Some(b'C'));
    assert_eq!(first.vram_byte(256 * 1024), None); // Capture only the VGA extent.

    device.io.write_mmio(0xb8000, 1, u64::from(b'Z'));
    device.io.write_port(0x3d4, 2, 0x000d);
    // A retained drawing location still targets its original page.
    device
        .card
        .borrow_mut()
        .framebuffer()
        .write_text_cell(&cell, cell.value());
    clock.advance_to(clock.at(Duration::from_millis(40)));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let second = frames.try_recv().unwrap();
    assert_eq!(second.vga().unwrap().display_start, 0);
    assert_eq!(second.vram_byte(0), Some(b'Z'));
    assert_eq!(second.vram_byte(8), Some(b'B'));
    assert_eq!(first.vram_byte(0), Some(b'A'));
    assert_eq!(first.vram_byte(8), Some(b'C'));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn board_clock_inputs_drive_the_shared_raster() {
    for (selection, frequency) in [(2, 32_514_000_u64), (3, 40_000_000)] {
        let clock = ManualClock::default();
        let device = ConnectedEt4000::new(&clock);
        device.io.write_port(0x3c2, 1, 0x43 | (selection << 2));
        let mut wait = pin!(device.card.borrow_mut().wait_for_retrace().unwrap());
        let mut cx = Context::from_waker(Waker::noop());
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        // The bare VGA preset reaches retrace at line 412, 800 dots per line.
        let expected = Duration::from_nanos((412 * 800 * 1_000_000_000_u64).div_ceil(frequency));
        assert_eq!(
            clock.next_deadline().map(|at| clock.elapsed(at)),
            Some(expected)
        );
        clock.advance_to(clock.at(expected));
        assert_eq!(device.io.read_port(0x3da, 1) & 8, 8);
        assert_eq!(
            wait.as_mut().poll(&mut cx),
            Poll::Ready(RetraceEvent::Started)
        );
    }
}
