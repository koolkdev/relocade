use super::{call, Firmware};
use relocade_machine::FarPointer;
use relocade_x86::{Segment, SegmentRegister};

const BUFFER: FarPointer = FarPointer {
    segment: 0x5000,
    offset: 0xfffc,
};

async fn state_services() {
    let machine = Firmware::new();
    let mut cpu = machine.cpu().await;
    let state = cpu.cpu_state();
    state.set_segment(
        Segment::Es,
        SegmentRegister::real_mode(Segment::Es, BUFFER.segment),
    );
    assert_eq!(call(&mut cpu, 0x4f02, 0x101, 0, 0).await, 0x004f);
    machine.io.write_port(0x3cd, 1, 0x75);
    machine.io.write_mmio(0xa0000, 1, 0x6a);
    machine.io.write_port(0x3d4, 1, 0x33);
    machine.io.read_port(0x3da, 1);
    machine.io.write_port(0x3c0, 1, 0x36);
    machine.io.write_port(0x3c8, 1, 9);
    machine.io.write_port(0x3c9, 1, 12);
    let core = machine.card.borrow().core_state();
    let dac = machine.card.borrow().dac_state();
    let extensions = machine.card.borrow().et4000_state();
    assert_eq!(call(&mut cpu, 0x4f04, BUFFER.offset, 15, 0).await, 0x004f);
    assert_eq!(state.bx(), 15); // 937 bytes, rounded to 64-byte blocks.
    machine
        .memory
        .view()
        .write_wrapping(BUFFER.wrapping_add(937), &[0xa5])
        .unwrap();
    assert_eq!(call(&mut cpu, 0x4f04, BUFFER.offset, 15, 1).await, 0x004f);
    let mut saved = [0; 938];
    machine
        .memory
        .view()
        .read_wrapping(BUFFER, &mut saved)
        .unwrap();
    assert_eq!(&saved[..6], b"RVBE\x01\x02");
    assert_eq!(saved[937], 0xa5);

    for mask in [1, 2, 4, 8, 15] {
        assert_eq!(call(&mut cpu, 0x4f02, 0x8103, 0, 0).await, 0x004f);
        let other_core = machine.card.borrow().core_state();
        let other_dac = machine.card.borrow().dac_state();
        let other_extensions = machine.card.borrow().et4000_state();
        assert_eq!(call(&mut cpu, 0x4f04, BUFFER.offset, mask, 2).await, 0x004f);
        let card = machine.card.borrow();
        assert_eq!(
            &card.core_state(),
            if mask & 1 != 0 { &core } else { &other_core }
        );
        assert_eq!(
            &card.dac_state(),
            if mask & 4 != 0 { &dac } else { &other_dac }
        );
        assert_eq!(
            &card.et4000_state(),
            if mask & 8 != 0 {
                &extensions
            } else {
                &other_extensions
            }
        );
        drop(card);
        assert_eq!(
            machine.memory.view().read_u8(0x449).unwrap(),
            if mask & 2 != 0 { 0x2e } else { 0x30 }
        );
    }
    assert_eq!(machine.io.read_port(0x3cd, 1), 0x75);
    machine.io.write_port(0x3cd, 1, 0x55);
    assert_eq!(machine.io.read_mmio(0xa0000, 1), 0x6a);

    let before_core = machine.card.borrow().core_state();
    let before_dac = machine.card.borrow().dac_state();
    let before_extensions = machine.card.borrow().et4000_state();
    let extension_offset = usize::from(u16::from_le_bytes(saved[14..16].try_into().unwrap()));
    for (offset, value) in [
        (5, 0),
        (5, 1), // Older ET4000 buffers have no RAMDAC command state.
        (6, 0x1f),
        (8, 17),
        (extension_offset + 13, 4),
        (extension_offset + 15, 5),
    ] {
        let mut corrupt = saved;
        corrupt[offset] = value;
        machine
            .memory
            .view()
            .write_wrapping(BUFFER, &corrupt)
            .unwrap();
        assert_eq!(call(&mut cpu, 0x4f04, BUFFER.offset, 15, 2).await, 0x014f);
        assert_eq!(machine.card.borrow().core_state(), before_core);
        assert_eq!(machine.card.borrow().dac_state(), before_dac);
        assert_eq!(machine.card.borrow().et4000_state(), before_extensions);
    }
    // Profile mismatches are rejected even for a VGA-only subset restore.
    saved[5] = 0;
    machine
        .memory
        .view()
        .write_wrapping(BUFFER, &saved)
        .unwrap();
    assert_eq!(call(&mut cpu, 0x4f04, BUFFER.offset, 1, 2).await, 0x014f);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn vbe_state_restores_selected_components_and_rejects_foreign_profiles() {
    pollster::block_on(state_services());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn vbe_state_restores_selected_components_and_rejects_foreign_profiles() {
    state_services().await;
}
