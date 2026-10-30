mod common;
use common::{boundary, crtc, ManualClock};
use relocade_machine::Device;
use relocade_vga::Vga;
use std::{rc::Rc, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn start(vga: &mut impl Device, address: u16) {
    let [high, low] = address.to_be_bytes();
    crtc(vga, 12, high);
    crtc(vga, 13, low);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn page_writes_before_at_and_after_retrace_select_the_following_display() {
    for offset in [-1, 0, 1] {
        let clock = Rc::new(ManualClock::default());
        let mut vga = Vga::with_clock(clock.clone());
        let source = vga.frame_source();
        vga.write_mmio(0xb8000, 2, 0x0441);
        vga.write_mmio(0xb8200, 2, 0x0142);
        let change_time = match offset {
            -1 => boundary(1) - Duration::from_nanos(1),
            0 => boundary(1),
            _ => boundary(1) + Duration::from_nanos(1),
        };
        clock.set(change_time);
        start(&mut vga, 0x100);
        assert_eq!(vga.snapshot().registers.crtc.display_start, 0);
        vga.write_port(0x3d4, 1, 12);
        assert_eq!(vga.read_port(0x3d5, 1), 1); // Programmed readback is immediate.

        if offset < 0 {
            clock.set(boundary(1));
            vga.read_port(0x3da, 1);
        }
        let first = source.current_capture().unwrap();
        let first_text = common::support::inspection::inspect(&first.display);
        assert_eq!(first.time, boundary(1));
        assert_eq!(first_text.start_address, if offset < 0 { 0x100 } else { 0 });
        assert_eq!(
            first_text.cells[0].character,
            if offset < 0 { b'B' } else { b'A' }
        );

        clock.set(boundary(2));
        vga.read_port(0x3da, 1);
        let second = source.current_capture().unwrap();
        assert_eq!(second.display.registers.crtc.display_start, 0x100);
        assert_eq!(
            first.display.registers.crtc.display_start,
            first_text.start_address
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn separate_start_bytes_can_straddle_the_latch_edge() {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    let source = vga.frame_source();
    clock.set(boundary(1) - Duration::from_nanos(1));
    crtc(&mut vga, 12, 0x12);
    clock.set(boundary(1));
    crtc(&mut vga, 13, 0x34);
    let first = source.current_capture().unwrap();
    assert_eq!(first.display.registers.crtc.display_start, 0x1200);
    assert_eq!(&first.display.registers.crtc.values[12..14], &[0x12, 0]);
    assert_eq!(vga.read_port(0x3d5, 1), 0x34);
    assert_eq!(vga.snapshot().registers.crtc.display_start, 0x1200);
    clock.set(boundary(2));
    vga.read_port(0x3da, 1);
    let next = source.current_capture().unwrap();
    assert_eq!(next.display.registers.crtc.display_start, 0x1234);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn late_service_latches_preceding_programming_before_a_new_write() {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    let source = vga.frame_source();
    start(&mut vga, 0x100);
    clock.set(boundary(100));
    start(&mut vga, 0x200);
    let latest = source.current_capture().unwrap();
    assert_eq!((latest.sequence, latest.time), (100, boundary(100)));
    assert_eq!(latest.display.registers.crtc.display_start, 0x100);
    assert_eq!(vga.snapshot().registers.crtc.display_start, 0x100);
    clock.set(boundary(101));
    vga.read_port(0x3da, 1);
    let next = source.current_capture().unwrap();
    assert_eq!(next.display.registers.crtc.display_start, 0x200);
    assert_eq!(latest.display.registers.crtc.display_start, 0x100);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn immediate_snapshots_copy_the_effective_latch_without_servicing_time() {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    let source = vga.frame_source();
    start(&mut vga, 0x1234);
    clock.set(boundary(1));
    let snapshot = vga.snapshot();
    assert_eq!(&snapshot.registers.crtc.values[12..14], &[0x12, 0x34]);
    assert_eq!(snapshot.registers.crtc.display_start, 0);
    assert!(source.current_capture().is_none());
    vga.read_port(0x3da, 1);
    let next = source.current_capture().unwrap();
    assert_eq!(next.display.registers.crtc.display_start, 0x1234);
    assert_eq!(snapshot.registers.crtc.display_start, 0);
}
