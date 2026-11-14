use super::Firmware;
use relocade_machine::FarPointer;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn indexed_mode_completion_applies_guest_tables_after_defaults_and_preserves_memory_on_request() {
    let machine = Firmware::new();
    machine.set_mode(0x30).unwrap();
    let defaults = machine.card.borrow().snapshot();
    let mut memory = machine.memory.view();
    let font16 = memory.read_u32(0x43 * 4).unwrap();

    // Relocate the primary table into guest RAM, retaining its mode-parameter
    // pointer. Install a graphics font and a secondary palette profile for 30h.
    let source = FarPointer::from_packed(memory.read_u32(0x4a8).unwrap());
    let mut primary = [0; 28];
    memory.read_wrapping(source, &mut primary).unwrap();
    primary[12..16].copy_from_slice(&0x5000_0100u32.to_le_bytes());
    primary[16..20].copy_from_slice(&0x5000_0040u32.to_le_bytes());
    memory.write(0x50000, &primary).unwrap();
    memory.write_u32(0x4a8, 0x5000_0000).unwrap();
    memory.write_u16(0x50040, 26).unwrap();
    memory.write_u32(0x5004a, 0x5000_0200).unwrap();
    memory
        .write(0x50100, &[27, 12, 0, 0, 0x10, 0, 0x70, 0x30, 0xff])
        .unwrap(); // 27 rows, 12-byte glyphs at 7000:1000.
    memory
        .write(
            0x50200,
            &[
                1, 0, 0, 0, // Underline follows the overridden font height.
                1, 0, 5, 0, 0, 3, 0, 0x50, // Attribute entry 5 from 5000:0300.
                1, 0, 111, 0, 0x10, 3, 0, 0x50, // DAC entry 111 from 5000:0310.
                0x30, 0xff,
            ],
        )
        .unwrap();
    memory.write_u8(0x50300, 0x2a).unwrap();
    memory.write(0x50310, &[6, 19, 31]).unwrap();

    machine.io.write_port(0x3cd, 1, 0xff);
    machine.io.write_mmio(0xaffff, 1, 0x5a);
    for (request, expected_byte) in [(0xb0, 0x5a), (0x30, 0)] {
        machine.set_mode(request).unwrap();
        assert_eq!(memory.read_u32(0x43 * 4).unwrap(), 0x7000_1000);
        assert_eq!(memory.read_u16(0x485).unwrap(), 12);
        assert_eq!(memory.read_u8(0x484).unwrap(), 26);
        assert_eq!(memory.read_u16(0x44a).unwrap(), 100);
        {
            let card = machine.card.borrow();
            assert_eq!(card.registers().attribute.values[5], 0x2a);
            assert_eq!(card.snapshot().dac.colors[111], [6, 19, 31]);
            assert_eq!(card.registers().crtc.registers.values[0x14] & 31, 11);
            assert_eq!(card.registers().sequencer.values[0], 3);
        }
        machine.io.write_port(0x3cd, 1, 0xff);
        assert_eq!(machine.io.read_mmio(0xaffff, 1), expected_byte);
    }

    // A mode outside the override lists still receives its own defaults.
    machine.set_mode(0x2e).unwrap();
    assert_eq!(memory.read_u32(0x43 * 4).unwrap(), font16);
    assert_eq!(memory.read_u16(0x485).unwrap(), 16);
    assert_eq!(memory.read_u8(0x484).unwrap(), 29);
    assert_eq!(machine.card.borrow().registers().attribute.values[5], 5);
    assert_eq!(
        machine.card.borrow().snapshot().dac.colors[111],
        defaults.dac.colors[111]
    );
}
