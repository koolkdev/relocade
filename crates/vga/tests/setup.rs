use crate::common::fixture::ConnectedVga;
use relocade_clock_test_support::ManualClock;
mod common;
use common::{boundary, poll_video};
use relocade_machine::Device;
use relocade_vga::Vga;
use std::{task::Context, task::Waker};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn disabled_addressing_preserves_registers_memory_and_access_phases() {
    let vga = ConnectedVga::new(ManualClock::new().clock());
    vga.io.write_mmio(0xb8000, 4, 0x2f42_0741);
    vga.io.write_port(0x3c8, 1, 4);
    vga.io.write_port(0x3c9, 1, 12); // Leave the DAC expecting green.
    vga.io.read_port(0x3da, 1);
    vga.io.write_port(0x3c0, 1, 0x01); // Palette writes enabled; leave it expecting data.
    let before = vga.snapshot();
    let registers = vga.device.borrow().registers().clone();

    vga.io.write_port(0x46e8, 1, 0);
    // Exercise every normal port: blocked reads must not advance DAC transfer
    // phases or reset the attribute address/data phase, and writes do nothing.
    for range in [0x3b4..=0x3b5, 0x3ba..=0x3ba, 0x3c0..=0x3cf, 0x3d4..=0x3da] {
        for port in range {
            assert_eq!(vga.io.read_port(port, 1), 0xff);
            vga.io.write_port(port, 1, 0xff);
        }
    }
    assert_eq!(vga.io.read_port(0x3c4, 4), u32::MAX);
    assert_eq!(vga.io.read_mmio(0xb8000, 8), u64::MAX);
    vga.io.write_mmio(0xb8000, 8, 0);
    let blocked = vga.snapshot();
    assert_eq!(vga.device.borrow().registers(), &registers);
    assert_eq!(blocked.scanout(), before.scanout());
    assert_eq!(blocked.dac, before.dac);
    for plane in 0..4 {
        assert!(std::ptr::eq(blocked.plane(plane), before.plane(plane)));
    }

    // Adapter control is write-only, but remains writable while disabled.
    assert_eq!(vga.io.read_port(0x46e8, 1), 0xff);
    vga.io.write_port(0x46e8, 1, 0x0e);
    assert_eq!(vga.io.read_mmio(0xb8000, 4), 0x2f42_0741);
    vga.io.write_port(0x3c9, 1, 23);
    vga.io.write_port(0x3c9, 1, 34);
    vga.io.write_port(0x3c0, 1, 5);
    let resumed = vga.snapshot();
    assert_eq!(resumed.dac.colors[4], [12, 23, 34]);
    assert_eq!(resumed.vga().unwrap().attributes[1], 5);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn setup_selects_the_sleep_bit_without_enabling_normal_access() {
    let vga = ConnectedVga::new(ManualClock::new().clock());
    vga.io.write_mmio(0xb8000, 2, 0x0741);
    let miscellaneous = vga.io.read_port(0x3cc, 1);
    assert_eq!(vga.io.read_port(0x102, 1), 0xff);
    vga.io.write_port(0x102, 1, 0); // Ignored outside setup.
    assert_eq!(vga.io.read_mmio(0xb8000, 2), 0x0741);

    // The selected setup-priority convention allows the option write with
    // either value of adapter enable, including the IBM BIOS's 16h write.
    for control in [0x16, 0x1e] {
        vga.io.write_port(0x46e8, 2, control);
        assert_eq!(vga.io.read_port(0x102, 2), 0xff01);
        assert_eq!(vga.io.read_port(0x3cc, 1), 0xff);
        assert_eq!(vga.io.read_mmio(0xb8000, 2), 0xffff);
        vga.io.write_port(0x3c2, 1, 0);
        vga.io.write_mmio(0xb8000, 2, 0);
        vga.io.write_port(0x102, 2, 0xfe); // Only the sleep bit is writable.
        assert_eq!(vga.io.read_port(0x102, 1), 0);
        vga.io.write_port(0x46e8, 1, 0x0e);
        assert_eq!(vga.io.read_port(0x3cc, 1), 0xff); // Sleep survives leaving setup.
        assert_eq!(vga.io.read_mmio(0xb8000, 2), 0xffff);
        vga.io.write_port(0x102, 1, 1); // Cannot wake it outside setup.
        assert_eq!(vga.io.read_port(0x3cc, 1), 0xff);
        vga.io.write_port(0x46e8, 1, 0x16);
        vga.io.write_port(0x102, 2, 0xff);
        assert_eq!(vga.io.read_port(0x102, 1), 1);
        vga.io.write_port(0x46e8, 1, 0x0e);
        assert_eq!(vga.io.read_port(0x3cc, 1), miscellaneous);
        assert_eq!(vga.io.read_mmio(0xb8000, 2), 0x0741);
    }

    // Adapter enable must not override Miscellaneous Output's RAM-only gate.
    vga.io.write_port(0x3c2, 1, miscellaneous & !2);
    vga.io.write_port(0x46e8, 1, 0);
    vga.io.write_port(0x46e8, 1, 0x0e);
    assert_eq!(vga.io.read_port(0x3cc, 1), miscellaneous & !2);
    assert_eq!(vga.io.read_mmio(0xb8000, 2), 0xffff);
    vga.io.write_port(0x3c2, 1, miscellaneous);
    assert_eq!(vga.io.read_mmio(0xb8000, 2), 0x0741);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn address_decoding_does_not_blank_stop_or_invalidate_display_captures() {
    let clock = ManualClock::default();
    let vga = common::text_vga(clock.clock());
    vga.io.write_mmio(0xb8000, 2, 0x0741);
    let source = vga.device.borrow().frames();
    let mut video = std::pin::pin!(Vga::run(vga.device.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    clock.advance_to(clock.at(boundary(1)));
    poll_video(video.as_mut(), &mut cx);
    let first = source.try_recv().unwrap();
    let registers = vga.device.borrow().registers().clone();
    vga.io.write_port(0x46e8, 1, 0);
    clock.advance_to(clock.at(boundary(2)));
    poll_video(video.as_mut(), &mut cx);
    assert!(source.is_empty());

    // Blink and capture timing keep running while CPU access is blocked.
    clock.advance_to(clock.at(boundary(16)));
    poll_video(video.as_mut(), &mut cx);
    let blinking = source.try_recv().unwrap();

    assert_ne!(
        blinking.vga().unwrap().attribute_blink_on,
        first.vga().unwrap().attribute_blink_on
    );
    assert_eq!(vga.device.borrow().registers(), &registers);
    assert_eq!(blinking.dac, first.dac);
    for plane in 0..4 {
        assert!(std::ptr::eq(blinking.plane(plane), first.plane(plane)));
    }
}
