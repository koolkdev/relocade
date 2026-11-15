use super::{call, Firmware, Segment, SegmentRegister};
use relocade_clock_test_support::ManualClock;
use relocade_vga::PixelFormat;
use std::{
    future::Future,
    pin::pin,
    task::{Context, Waker},
};

async fn palette_blocks() {
    let machine = Firmware::new();
    let mut cpu = machine.cpu().await;
    assert_eq!(call(&mut cpu, 0x4f02, 0x101, 0, 0).await, 0x4f);
    machine.io.write_port(0x3cd, 1, 0x11);
    machine.io.write_mmio(0xa0100, 4, 0xa704_0302);
    machine.io.write_port(0x3cd, 1, 0x13); // Read bank one, write bank three.
    let state = cpu.cpu_state();
    state.set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0xa000));
    state.set_di(0x100);
    machine.io.write_port(0x3c6, 1, 0x5a);
    for _ in 0..4 {
        machine.io.read_port(0x3c6, 1);
    }

    // The input lives in this card's own VRAM; no borrow may span its read.
    assert_eq!(call(&mut cpu, 0x4f09, 0, 1, 17).await, 0x4f);
    assert_eq!(machine.card.borrow().registers().dac.colors[17], [4, 3, 2]);
    assert_eq!(machine.card.borrow().registers().dac.address(), 18);
    assert_eq!(machine.io.read_port(0x3cd, 1), 0x13);
    assert_eq!(
        machine.io.read_port(0x3c6, 1),
        0x5a,
        "palette programming ends command access"
    );
    assert_eq!(
        machine.card.borrow().packed_layout().unwrap().format(),
        PixelFormat::Indexed8
    );

    let dac = machine.card.borrow().dac_state();
    assert_eq!(call(&mut cpu, 0x4f09, 1, 1, 17).await, 0x4f);
    assert_eq!(machine.card.borrow().dac_state(), dac);
    assert_eq!(machine.io.read_port(0x3cd, 1), 0x13);
    assert_eq!(
        machine.io.read_mmio(0xa0100, 4),
        0xa704_0302,
        "input bank is untouched"
    );
    machine.io.write_port(0x3cd, 1, 0x33);
    assert_eq!(
        machine.io.read_mmio(0xa0100, 4),
        0x0004_0302,
        "output uses the write bank"
    );

    assert_eq!(call(&mut cpu, 0x4f02, 0x110, 0, 0).await, 0x4f);
    let dac = machine.card.borrow().dac_state();
    let extensions = machine.card.borrow().et4000_state();
    for operation in [0, 1] {
        assert_eq!(call(&mut cpu, 0x4f09, operation, 1, 17).await, 0x34f);
        assert_eq!(machine.card.borrow().dac_state(), dac);
        assert_eq!(machine.card.borrow().et4000_state(), extensions);
    }
}

async fn retrace_update() {
    let clock = ManualClock::default();
    let machine = Firmware::with_clock(clock.clock());
    let mut cpu = machine.cpu().await;
    assert_eq!(call(&mut cpu, 0x4f02, 0x101, 0, 0).await, 0x4f);
    let state = cpu.cpu_state();
    state.set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x5000));
    state.set_di(0x100);
    state.set_eflags(0x443); // IF starts clear; the suspended hook temporarily enables it.
    machine
        .memory
        .view()
        .write(0x50100, &[4, 5, 6, 0xee])
        .unwrap();
    let dac = machine.card.borrow().dac_state();
    let mut request = pin!(call(&mut cpu, 0x4f09, 0x1280, 1, 17));
    assert!(request
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    assert!(
        state.interrupts_enabled(),
        "the hooked wait permits guest IRQs"
    );
    assert_eq!(machine.card.borrow().dac_state(), dac);
    clock.advance_to(clock.next_deadline().expect("palette retrace deadline"));
    assert_eq!(request.await, 0x4f);
    assert_eq!(machine.card.borrow().registers().dac.colors[17], [6, 5, 4]);
    assert_eq!(state.bx(), 0x1280);
    assert_eq!([state.cx(), state.dx(), state.di()], [1, 17, 0x100]);
    assert_eq!(state.segment(Segment::Es).selector, 0x5000);
    assert_eq!(state.sp(), 0x8000);
    assert_eq!(state.eflags(), 0x443);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn palette_transfers_use_independent_banks_and_the_selected_ramdac() {
    pollster::block_on(palette_blocks());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn palette_transfers_use_independent_banks_and_the_selected_ramdac() {
    palette_blocks().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn hooked_palette_update_waits_for_retrace_and_preserves_the_callers_registers() {
    pollster::block_on(retrace_update());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn hooked_palette_update_waits_for_retrace_and_preserves_the_callers_registers() {
    retrace_update().await;
}
