use super::*;

fn unlock(io: &IoBus, mode_port: u16) {
    io.write_port(0x3bf, 1, 3);
    io.write_port(mode_port, 1, 0xa0);
}

fn crtc_read(io: &IoBus, index: u8) -> u32 {
    io.write_port(0x3d4, 1, u32::from(index));
    io.read_port(0x3d5, 1)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn detection_unlocks_extensions_without_aliasing_standard_registers() {
    let device = ConnectedEt4000::new(&ManualClock::default());
    let io = &device.io;
    assert_eq!(io.read_port(0x3c2, 1) & 0x60, 0);
    io.write_port(0x3d4, 2, 0x5532);
    assert_eq!(crtc_read(io, 0x32), 0);
    unlock(io, 0x3d8);
    assert_eq!(io.read_port(0x3c2, 1) & 0x60, 0x60);
    for pattern in [0x55, 0xaa] {
        io.write_port(0x3d4, 2, (pattern << 8) | 0x32);
        assert_eq!(crtc_read(io, 0x32), pattern);
    }
    io.write_port(0x3c4, 2, 0xff06);
    assert_eq!(io.read_port(0x3c5, 1), 6);
    io.read_port(0x3da, 1);
    io.write_port(0x3c0, 1, 0x36);
    io.write_port(0x3c0, 1, 0xff);
    assert_eq!(io.read_port(0x3c1, 1), 0xf0);
    io.write_port(0x3bf, 1, 1);
    io.write_port(0x3d8, 1, 0);
    io.write_port(0x3d4, 2, 0x0032);
    assert_eq!(crtc_read(io, 0x32), 0xaa);
    io.write_port(0x3c4, 2, 0x0006);
    assert_eq!(io.read_port(0x3c5, 1), 6);
    io.read_port(0x3da, 1);
    io.write_port(0x3c0, 1, 0x36);
    io.write_port(0x3c0, 1, 0);
    assert_eq!(io.read_port(0x3c1, 1), 0xf0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn address_and_overflow_extensions_have_their_own_protection_rules() {
    let device = ConnectedEt4000::new(&ManualClock::default());
    let io = &device.io;
    io.write_port(0x3d4, 2, 0xff33); // Writable with KEY clear; reserved high nibble.
    assert_eq!(crtc_read(io, 0x33), 0x0f);
    io.write_port(0x3d4, 2, 0x0011); // Timing protection off.
    io.write_port(0x3d4, 2, 0x6f35);
    assert_eq!(crtc_read(io, 0x35), 0x6f);
    io.write_port(0x3d4, 2, 0x8011);
    io.write_port(0x3d4, 2, 0x9035);
    assert_eq!(crtc_read(io, 0x35), 0xff);
    io.write_port(0x3d4, 2, 0x0035);
    assert_eq!(crtc_read(io, 0x35), 0x6f);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn segment_access_survives_relocking_but_not_synchronous_reset() {
    let device = ConnectedEt4000::new(&ManualClock::default());
    let io = &device.io;
    io.write_port(0x3cd, 1, 0x12);
    assert_eq!(io.read_port(0x3cd, 1), 0xff);
    unlock(io, 0x3d8);
    io.write_port(0x3bf, 1, 1);
    io.write_port(0x3d8, 1, 0);
    // The second byte still reaches the next physical port, Graphics Index.
    io.write_port(0x3cd, 2, 0x0421);
    assert_eq!(io.read_port(0x3cd, 2), 0x0421);
    io.write_port(0x3c4, 2, 0x0100);
    io.write_port(0x3c4, 2, 0x0300);
    io.write_port(0x3cd, 1, 0xff);
    assert_eq!(io.read_port(0x3cd, 1), 0xff);
    unlock(io, 0x3d8);
    assert_eq!(io.read_port(0x3cd, 1), 0x21);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn active_address_group_and_enable_port_remain_accessible() {
    let device = ConnectedEt4000::new(&ManualClock::default());
    let io = &device.io;
    io.write_port(0x3c2, 1, 0x42);
    unlock(io, 0x3d8); // Inactive color port cannot set KEY.
    assert_eq!(io.read_port(0x3c2, 1) & 0x60, 0);
    io.write_port(0x3b8, 1, 0xa0);
    assert_eq!(io.read_port(0x3c2, 1) & 0x60, 0x60);
    io.write_port(0x3b4, 2, 0x0034); // Select 3C3h for adapter enable.
    assert_eq!(io.read_port(0x46e8, 1), 0xff);
    io.write_port(0x3c3, 1, 0);
    assert_eq!(io.read_port(0x3b4, 2), 0xffff);
    io.write_port(0x46e8, 1, 8); // Inactive enable address does nothing.
    assert_eq!(io.read_port(0x3b4, 2), 0xffff);
    assert_eq!(io.read_port(0x3c3, 1), 0);
    io.write_port(0x3c3, 1, 1);
    assert_eq!(io.read_port(0x3b4, 2), 0x0034);
    io.write_port(0x3b4, 2, 0x0834);
    io.write_port(0xa6e8, 1, 0);
    assert_eq!(io.read_port(0x3b4, 2), 0xffff);
    io.write_port(0xf6e8, 1, 8);
    assert_eq!(io.read_port(0x3b4, 2), 0x0834);
}
