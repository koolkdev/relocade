use super::{call, Firmware, Segment, SegmentRegister};
use relocade_vga::PixelFormat;

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
