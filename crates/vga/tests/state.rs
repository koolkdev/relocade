use crate::common::fixture::ConnectedVga;
use relocade_clock_test_support::ManualClock;
mod common;
use common::{crtc, dots, support::svga, svga_boundary};
use relocade_vga::{DacState, DispiState, PixelFormat, RetraceEvent, Scanout, VgaCoreState};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn vga_core_restore_preserves_vram_and_recovers_latches_reset_and_access_phases() {
    let vga = ConnectedVga::new(ManualClock::new().clock());
    vga.io.write_port(0x3c4, 2, 0x0604);
    vga.io.write_port(0x3ce, 2, 0x0406);
    vga.io.write_port(0x3ce, 2, 0x0005);
    for (plane, value) in [0x12, 0x34, 0x56, 0x78].into_iter().enumerate() {
        vga.io.write_port(0x3c4, 2, ((1 << plane) << 8) | 2);
        vga.io.write_mmio(0xa0010, 1, value);
        vga.io.write_mmio(0xaffff, 1, value + 1);
    }
    vga.io.read_mmio(0xa0010, 1);
    crtc(&vga.io, 0x0c, 1);
    crtc(&vga.io, 0x0d, 0x23);
    vga.io.write_port(0x3c4, 2, 0x0100); // Hold synchronous reset.
    vga.io.write_port(0x3c4, 1, 7);
    vga.io.write_port(0x3ce, 1, 15);
    vga.io.write_port(0x3d4, 1, 31);
    vga.io.read_port(0x3da, 1);
    vga.io.write_port(0x3c0, 1, 0x12); // Next write is attribute data.
    let registers = vga.device.borrow().registers().clone();
    let before = vga.snapshot();
    let state = vga.device.borrow().core_state();
    let saved = VgaCoreState::from_bytes(&state.to_bytes()).unwrap();
    assert_eq!(saved, state);
    vga.io.read_mmio(0xa0020, 1); // Disturb the hidden CPU latches.
    vga.io.write_port(0x3c4, 2, 0x0300);
    crtc(&vga.io, 0x0d, 0x45);
    vga.io.read_port(0x3da, 1);

    vga.io.with_context(|context| {
        vga.device
            .borrow_mut()
            .restore_state(context, Some(saved), None, None);
    });
    assert_eq!(vga.device.borrow().registers(), &registers);
    assert_eq!(vga.device.borrow().core_state(), state);
    assert_eq!(vga.device.borrow().registers().crtc.display_start, 0);
    for plane in 0..4 {
        assert_eq!(vga.snapshot().plane(plane), before.plane(plane));
    }
    vga.io.write_port(0x3c0, 1, 7);
    assert_eq!(vga.device.borrow().registers().attribute.values[0x12], 7);
    vga.io.write_port(0x3c4, 2, 0x0f02);
    vga.io.write_port(0x3ce, 2, 0x0105);
    vga.io.write_mmio(0xa0020, 1, 0);
    for (plane, value) in [0x12, 0x34, 0x56, 0x78].into_iter().enumerate() {
        assert_eq!(vga.snapshot().plane(plane)[0x20], value);
        assert_eq!(before.plane(plane)[0x20], 0);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dac_restore_continues_partial_transfers_and_preserves_other_state_classes() {
    for reading in [false, true] {
        let vga = ConnectedVga::new(ManualClock::new().clock());
        svga::enable(&vga.io);
        vga.io.write_port(0x3c8, 1, 255);
        for value in [10, 20, 30] {
            vga.io.write_port(0x3c9, 1, value);
        }
        vga.io.write_port(0x3c6, 1, 0xa5);
        vga.io.read_port(0x3da, 1);
        vga.io.write_port(0x3c0, 1, 0x14);
        vga.io.write_port(0x3c0, 1, 11);
        vga.io
            .write_port(if reading { 0x3c7 } else { 0x3c8 }, 1, 255);
        if reading {
            vga.io.read_port(0x3c9, 1);
        } else {
            vga.io.write_port(0x3c9, 1, 10);
        }
        let core = vga.device.borrow().core_state();
        let dispi = vga.device.borrow().dispi_state();
        let saved = vga.device.borrow().dac_state();
        let decoded = DacState::from_bytes(&saved.to_bytes()).unwrap();
        assert_eq!(decoded, saved);
        vga.io.write_port(0x3c8, 1, 255);
        vga.io.write_port(0x3c9, 1, 63);

        vga.io.with_context(|context| {
            vga.device
                .borrow_mut()
                .restore_state(context, None, Some(decoded), None);
        });
        assert_eq!(vga.device.borrow().core_state(), core);
        assert_eq!(vga.device.borrow().dispi_state(), dispi);
        assert_eq!(vga.device.borrow().dac_state(), saved);
        if reading {
            assert_eq!(vga.io.read_port(0x3c9, 1), 20);
            assert_eq!(vga.io.read_port(0x3c9, 1), 30);
            assert_eq!(vga.io.read_port(0x3c8, 1), 1);
        } else {
            vga.io.write_port(0x3c9, 1, 21);
            vga.io.write_port(0x3c9, 1, 31);
            assert_eq!(vga.snapshot().dac.colors[255], [10, 21, 31]);
            assert_eq!(vga.io.read_port(0x3c8, 1), 0);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dispi_restore_recovers_exact_flags_layout_bank_and_disabled_staging() {
    let vga = ConnectedVga::new(ManualClock::new().clock());
    for (register, value) in [
        (1, 800),
        (2, 600),
        (3, 8),
        (4, 1),
        (6, 1024),
        (8, 24),
        (9, 600),
        (5, 63),
    ] {
        svga::set(&vga.io, register, value);
    }
    vga.io.write_mmio(0xaffff, 1, 0xab);
    vga.io.write_port(0x1ce, 2, 0x1234);
    let saved = vga.device.borrow().dispi_state();
    let retained = vga.snapshot();
    for (register, value) in [(4, 0), (1, 3), (2, 17), (3, 15), (5, 7)] {
        svga::set(&vga.io, register, value);
    }
    let staged = vga.device.borrow().dispi_state();
    assert_eq!(DispiState::from_bytes(&staged.to_bytes()).unwrap(), staged);
    let decoded = DispiState::from_bytes(&saved.to_bytes()).unwrap();

    vga.io.with_context(|context| {
        vga.device
            .borrow_mut()
            .restore_state(context, None, None, Some(decoded));
    });
    assert_eq!(vga.device.borrow().dispi_state(), saved);
    assert_eq!(vga.io.read_port(0x1ce, 2), 0x1234);
    assert_eq!(svga::get(&vga.io, 4), 1);
    assert_eq!(vga.io.read_mmio(0xaffff, 1), 0xab);
    assert_eq!(vga.snapshot().scanout(), retained.scanout());

    vga.io.with_context(|context| {
        vga.device
            .borrow_mut()
            .restore_state(context, None, None, Some(staged.clone()));
    });
    assert_eq!(vga.device.borrow().dispi_state(), staged);
    assert!(matches!(vga.snapshot().scanout(), Scanout::Vga(_)));
    assert!(matches!(retained.scanout(), Scanout::Svga(layout)
        if layout.size() == [800, 600] && layout.format() == PixelFormat::Indexed8));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn restored_programming_services_old_capture_and_replaces_only_changed_timing() {
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(ConnectedVga::new(clock.clock()));
    svga::enable(&vga.io);
    let original = vga.device.borrow().dispi_state();
    for (register, value) in [(4, 0), (1, 800), (2, 600), (4, 0x81)] {
        svga::set(&vga.io, register, value);
    }
    let larger = vga.device.borrow().dispi_state();

    vga.io.with_context(|context| {
        vga.device
            .borrow_mut()
            .restore_state(context, None, None, Some(original));
    });
    let frames = vga.device.borrow().frames();
    let mut wait = pin!(vga.device.borrow_mut().wait_for_retrace().unwrap());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    clock.advance_to(clock.at(Duration::from_millis(1)));
    let dac = vga.device.borrow().dac_state();

    vga.io.with_context(|context| {
        vga.device
            .borrow_mut()
            .restore_state(context, None, Some(dac), None);
    });
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    clock.advance_to(clock.at(svga_boundary(1)));

    vga.io.with_context(|context| {
        vga.device
            .borrow_mut()
            .restore_state(context, None, None, Some(larger));
    });
    common::service_video(video.as_mut());
    assert_eq!(
        wait.as_mut().poll(&mut cx),
        Poll::Ready(RetraceEvent::Reconfigured)
    );
    let old = frames.try_recv().unwrap();
    assert!(matches!(old.scanout(), Scanout::Svga(layout)
        if layout.size() == [640, 480] && layout.format() == PixelFormat::Indexed8));
    let mut next = pin!(vga.device.borrow_mut().wait_for_retrace().unwrap());
    assert!(next.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        clock.next_deadline().map(|at| clock.elapsed(at)),
        Some(svga_boundary(1) + dots(601 * 1056, 40_000_000))
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn malformed_component_bytes_are_rejected_before_restoration() {
    let vga = ConnectedVga::new(ManualClock::new().clock());
    for (offset, value) in [(3, 2), (4, 8), (8, 2), (15, 4)] {
        let mut bytes = vga.device.borrow().core_state().to_bytes();
        bytes[offset] = value;
        assert!(VgaCoreState::from_bytes(&bytes).is_none());
    }
    for (offset, value) in [(2, 3), (3, 2), (4, 64), (772, 16)] {
        let mut bytes = vga.device.borrow().dac_state().to_bytes();
        bytes[offset] = value;
        assert!(DacState::from_bytes(&bytes).is_none());
    }
    svga::enable(&vga.io);
    for (offset, value) in [(6, 15), (8, 2), (10, 64), (12, 1)] {
        let mut bytes = vga.device.borrow().dispi_state().to_bytes();
        bytes[offset..offset + 2].copy_from_slice(&u16::to_le_bytes(value));
        assert!(DispiState::from_bytes(&bytes).is_none());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn direct_color_state_keeps_byte_pitch_and_same_resolution_raster_phase() {
    for (bits, pitch, rows) in [(16, 2048u16, 2048), (32, 4096u16, 1024)] {
        let clock = ManualClock::default();
        let vga = ConnectedVga::new(clock.clock());
        svga::set_mode(&vga.io, 640, 480, bits);
        svga::set(&vga.io, 6, 1024);
        svga::set(&vga.io, 8, 384);
        svga::set(&vga.io, 9, rows - 480);
        svga::write_byte(&vga.io, 4 * 1024 * 1024 - 1, 0xa5);
        let retained = vga.snapshot();
        assert_eq!(retained.vram_byte(4 * 1024 * 1024 - 1), Some(0xa5));
        let state = vga.device.borrow().dispi_state();
        let mut bytes = state.to_bytes();
        assert_eq!(&bytes[12..14], &pitch.to_le_bytes());
        let decoded = DispiState::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, state);
        bytes[12..14].copy_from_slice(&(pitch + 1).to_le_bytes());
        assert!(DispiState::from_bytes(&bytes).is_none());
        svga::enable(&vga.io);
        clock.advance_to(clock.at(Duration::from_millis(1)));

        vga.io.with_context(|context| {
            vga.device
                .borrow_mut()
                .restore_state(context, None, None, Some(decoded));
        });

        assert_eq!(vga.device.borrow().dispi_state(), state);
        assert_eq!(svga::get(&vga.io, 6), 1024);
        assert_eq!(svga::get(&vga.io, 7), u32::from(rows));
        let mut wait = pin!(vga.device.borrow_mut().wait_for_retrace().unwrap());
        assert!(wait
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        assert_eq!(
            clock.next_deadline().map(|at| clock.elapsed(at)),
            Some(svga_boundary(1))
        );
        assert_eq!(retained.vram_byte(4 * 1024 * 1024 - 1), Some(0xa5));
    }
}
