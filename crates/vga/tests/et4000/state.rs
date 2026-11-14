use super::{scanout, ConnectedEt4000, ManualClock};
use relocade_vga::{DacState, Et4000Ax, Et4000State, VgaCoreState};
use std::time::Duration;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn restoring_card_state_preserves_protocols_banks_and_pending_display_start() {
    let clock = ManualClock::default();
    let device = ConnectedEt4000::new(&clock);
    let io = &device.io;
    scanout::indexed_640(io);
    io.write_port(0x3cd, 1, 0x75);
    io.write_mmio(0xa0000, 1, 0x6a);
    io.write_port(0x3d4, 2, 0x0133);
    clock.advance_to(clock.at(Duration::from_millis(20)));
    io.read_port(0x3da, 1); // Latch high counter one.
    io.write_port(0x3d4, 2, 0x0233); // Program high two, pending the next retrace.
    io.write_port(0x3d4, 1, 0x3f); // Legal extended index, including an unimplemented slot.
    io.write_port(0x3c4, 1, 7);
    io.write_port(0x3c0, 1, 0x36); // Extension data is the next Attribute write.
    io.write_port(0x3c8, 1, 9);
    io.write_port(0x3c9, 1, 12); // Next component is green.
    io.write_port(0x3bf, 1, 0); // Relock without disabling previously enabled banks.
    let core = device.card.borrow().core_state();
    let dac = device.card.borrow().dac_state();
    let extended = device.card.borrow().et4000_state();
    let latched = scanout::layout(&device).display_start();
    assert_eq!(latched, 0x40000);
    assert_eq!(
        device
            .card
            .borrow()
            .packed_layout()
            .unwrap()
            .display_start(),
        0x80000
    );
    assert!(VgaCoreState::from_bytes(&core.to_bytes()).is_none());
    let core = Et4000Ax::decode_core_state(&core.to_bytes()).unwrap();
    let dac = DacState::from_bytes(&dac.to_bytes()).unwrap();
    let extended = Et4000State::from_bytes(&extended.to_bytes()).unwrap();

    io.write_port(0x3bf, 1, 3);
    io.write_port(0x3cd, 1, 0);
    io.write_port(0x3d4, 2, 0x0033);
    io.write_port(0x3c9, 1, 63);
    io.write_port(0x46e8, 1, 0);
    io.with_context(|context| {
        device.card.borrow_mut().restore_state(
            context,
            Some(core.clone()),
            Some(dac.clone()),
            Some(extended.clone()),
        )
    });
    assert_eq!(device.card.borrow().core_state(), core);
    assert_eq!(device.card.borrow().dac_state(), dac);
    assert_eq!(device.card.borrow().et4000_state(), extended);
    assert_eq!(scanout::layout(&device).display_start(), latched);
    assert_eq!(io.read_port(0x3cd, 1), 0x75);
    io.write_port(0x3cd, 1, 0x55); // Restored segment access survives the locked key.
    assert_eq!(io.read_mmio(0xa0000, 1), 0x6a);
    io.write_port(0x3c9, 1, 23);
    io.write_port(0x3c9, 1, 34);
    assert_eq!(device.card.borrow().snapshot().dac.colors[9], [12, 23, 34]);
    io.write_port(0x3bf, 1, 3);
    io.write_port(0x3c0, 1, 0x80);
    assert_eq!(io.read_port(0x3c1, 1), 0x80);
    // The cached status reader still resets the restored Attribute protocol.
    io.read_port(0x3da, 1);
    io.write_port(0x3c0, 1, 0x30);
    assert_eq!(io.read_port(0x3c0, 1), 0x30);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn extension_state_rejects_noncanonical_bits_before_restoration() {
    let device = ConnectedEt4000::new(&ManualClock::default());
    let saved = device.card.borrow().et4000_state().to_bytes();
    for (offset, value) in [(1, 0x10), (6, 1), (8, 1), (9, 4), (12, 2), (13, 4), (15, 5)] {
        let mut bytes = saved;
        bytes[offset] = value;
        assert!(Et4000State::from_bytes(&bytes).is_none());
    }
    let mut core = device.card.borrow().core_state().to_bytes();
    core[6] = 0x40;
    assert!(Et4000Ax::decode_core_state(&core).is_none());
}
