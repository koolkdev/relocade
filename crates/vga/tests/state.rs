mod common;
use common::{crtc, dots, support::svga, svga_boundary, ManualClock};
use relocade_machine::Device;
use relocade_vga::{DacState, DispiState, RetraceEvent, Scanout, SvgaMode, Vga, VgaCoreState};
use std::{
    future::Future,
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn vga_core_restore_preserves_vram_and_recovers_latches_reset_and_access_phases() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    vga.write_port(0x3c4, 2, 0x0604);
    vga.write_port(0x3ce, 2, 0x0406);
    vga.write_port(0x3ce, 2, 0x0005);
    for (plane, value) in [0x12, 0x34, 0x56, 0x78].into_iter().enumerate() {
        vga.write_port(0x3c4, 2, ((1 << plane) << 8) | 2);
        vga.write_mmio(0xa0010, 1, value);
        vga.write_mmio(0xaffff, 1, value + 1);
    }
    vga.read_mmio(0xa0010, 1);
    crtc(&mut vga, 0x0c, 1);
    crtc(&mut vga, 0x0d, 0x23);
    vga.write_port(0x3c4, 2, 0x0100); // Hold synchronous reset.
    vga.write_port(0x3c4, 1, 7);
    vga.write_port(0x3ce, 1, 15);
    vga.write_port(0x3d4, 1, 31);
    vga.read_port(0x3da, 1);
    vga.write_port(0x3c0, 1, 0x12); // Next write is attribute data.
    let registers = vga.registers().clone();
    let before = vga.snapshot();
    let state = vga.core_state();
    let saved = VgaCoreState::from_bytes(&state.to_bytes()).unwrap();
    assert_eq!(saved, state);
    vga.read_mmio(0xa0020, 1); // Disturb the hidden CPU latches.
    vga.write_port(0x3c4, 2, 0x0300);
    crtc(&mut vga, 0x0d, 0x45);
    vga.read_port(0x3da, 1);
    vga.restore_state(Some(saved), None, None);
    assert_eq!(vga.registers(), &registers);
    assert_eq!(vga.core_state(), state);
    assert_eq!(vga.registers().crtc.display_start, 0);
    for plane in 0..4 {
        assert_eq!(vga.snapshot().plane(plane), before.plane(plane));
    }
    vga.write_port(0x3c0, 1, 7);
    assert_eq!(vga.registers().attribute.values[0x12], 7);
    vga.write_port(0x3c4, 2, 0x0f02);
    vga.write_port(0x3ce, 2, 0x0105);
    vga.write_mmio(0xa0020, 1, 0);
    for (plane, value) in [0x12, 0x34, 0x56, 0x78].into_iter().enumerate() {
        assert_eq!(vga.snapshot().plane(plane)[0x20], value);
        assert_eq!(before.plane(plane)[0x20], 0);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dac_restore_continues_partial_transfers_and_preserves_other_state_classes() {
    for reading in [false, true] {
        let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
        svga::enable(&mut vga);
        vga.write_port(0x3c8, 1, 255);
        for value in [10, 20, 30] {
            vga.write_port(0x3c9, 1, value);
        }
        vga.write_port(0x3c6, 1, 0xa5);
        vga.read_port(0x3da, 1);
        vga.write_port(0x3c0, 1, 0x14);
        vga.write_port(0x3c0, 1, 11);
        vga.write_port(if reading { 0x3c7 } else { 0x3c8 }, 1, 255);
        if reading {
            vga.read_port(0x3c9, 1);
        } else {
            vga.write_port(0x3c9, 1, 10);
        }
        let core = vga.core_state();
        let dispi = vga.dispi_state();
        let saved = vga.dac_state();
        let decoded = DacState::from_bytes(&saved.to_bytes()).unwrap();
        assert_eq!(decoded, saved);
        vga.write_port(0x3c8, 1, 255);
        vga.write_port(0x3c9, 1, 63);
        vga.restore_state(None, Some(decoded), None);
        assert_eq!(vga.core_state(), core);
        assert_eq!(vga.dispi_state(), dispi);
        assert_eq!(vga.dac_state(), saved);
        if reading {
            assert_eq!(vga.read_port(0x3c9, 1), 20);
            assert_eq!(vga.read_port(0x3c9, 1), 30);
            assert_eq!(vga.read_port(0x3c8, 1), 1);
        } else {
            vga.write_port(0x3c9, 1, 21);
            vga.write_port(0x3c9, 1, 31);
            assert_eq!(vga.snapshot().dac.colors[255], [10, 21, 31]);
            assert_eq!(vga.read_port(0x3c8, 1), 0);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dispi_restore_recovers_exact_flags_layout_bank_and_disabled_staging() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
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
        svga::set(&mut vga, register, value);
    }
    vga.write_mmio(0xaffff, 1, 0xab);
    vga.write_port(0x1ce, 2, 0x1234);
    let saved = vga.dispi_state();
    let retained = vga.snapshot();
    for (register, value) in [(4, 0), (1, 3), (2, 17), (3, 15), (5, 7)] {
        svga::set(&mut vga, register, value);
    }
    let staged = vga.dispi_state();
    assert_eq!(DispiState::from_bytes(&staged.to_bytes()).unwrap(), staged);
    let decoded = DispiState::from_bytes(&saved.to_bytes()).unwrap();
    vga.restore_state(None, None, Some(decoded));
    assert_eq!(vga.dispi_state(), saved);
    assert_eq!(vga.read_port(0x1ce, 2), 0x1234);
    assert_eq!(svga::get(&mut vga, 4), 1);
    assert_eq!(vga.read_mmio(0xaffff, 1), 0xab);
    assert_eq!(vga.snapshot().scanout(), retained.scanout());
    vga.restore_state(None, None, Some(staged.clone()));
    assert_eq!(vga.dispi_state(), staged);
    assert!(matches!(vga.snapshot().scanout(), Scanout::Vga(_)));
    assert!(
        matches!(retained.scanout(), Scanout::Svga(layout) if layout.mode() == SvgaMode::Indexed800x600)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn restored_programming_services_old_capture_and_replaces_only_changed_timing() {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    svga::enable(&mut vga);
    let original = vga.dispi_state();
    for (register, value) in [(4, 0), (1, 800), (2, 600), (4, 0x81)] {
        svga::set(&mut vga, register, value);
    }
    let larger = vga.dispi_state();
    vga.restore_state(None, None, Some(original));
    let frames = vga.frame_source();
    let mut wait = pin!(vga.wait_for_retrace().unwrap());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    clock.set(Duration::from_millis(1));
    vga.restore_state(None, Some(vga.dac_state()), None);
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    clock.set(svga_boundary(1));
    vga.restore_state(None, None, Some(larger));
    assert_eq!(
        wait.as_mut().poll(&mut cx),
        Poll::Ready(RetraceEvent::Reprogrammed)
    );
    let old = frames.current_capture().unwrap();
    assert!(
        matches!(old.display.scanout(), Scanout::Svga(layout) if layout.mode() == SvgaMode::Indexed640x480)
    );
    let mut next = pin!(vga.wait_for_retrace().unwrap());
    assert!(next.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        clock.deadline.get(),
        Some(svga_boundary(1) + dots(601 * 1056, 40_000_000))
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn malformed_component_bytes_are_rejected_before_restoration() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    for (offset, value) in [(3, 2), (4, 8), (8, 2), (15, 4)] {
        let mut bytes = vga.core_state().to_bytes();
        bytes[offset] = value;
        assert!(VgaCoreState::from_bytes(&bytes).is_none());
    }
    for (offset, value) in [(2, 3), (3, 2), (4, 64), (772, 16)] {
        let mut bytes = vga.dac_state().to_bytes();
        bytes[offset] = value;
        assert!(DacState::from_bytes(&bytes).is_none());
    }
    svga::enable(&mut vga);
    for (offset, value) in [(6, 15), (8, 2), (10, 64), (12, 1)] {
        let mut bytes = vga.dispi_state().to_bytes();
        bytes[offset..offset + 2].copy_from_slice(&u16::to_le_bytes(value));
        assert!(DispiState::from_bytes(&bytes).is_none());
    }
}
