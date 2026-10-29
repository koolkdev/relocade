use relocade_machine::Device;
use relocade_vga::{TextCell, Vga, CRTC_DATA_PORT, CRTC_INDEX_PORT, TEXT_MEMORY_START};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn register(vga: &mut Vga, index: u8, value: u8) {
    vga.write_port(
        CRTC_INDEX_PORT,
        2,
        u32::from(u16::from_le_bytes([index, value])),
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn text_aperture_selects_planes_and_scanout_uses_full_crtc_addresses() {
    let mut vga = Vga::new();
    vga.write_mmio(TEXT_MEMORY_START, 4, 0x2f42_1e41);
    vga.write_mmio(TEXT_MEMORY_START + 1, 1, 0x17);
    assert_eq!(vga.read_mmio(TEXT_MEMORY_START, 4), 0x2f42_1741);
    let first = vga.screen();
    assert_eq!(
        first.cells[0],
        TextCell {
            character: b'A',
            attribute: 0x17
        }
    );
    assert_eq!(
        first.cells[1],
        TextCell {
            character: b'B',
            attribute: 0x2f
        }
    );

    // The final CPU-accessible cell precedes video memory outside that aperture.
    vga.write_mmio(TEXT_MEMORY_START + 0x7ffe, 2, 0x3e5a);
    register(&mut vga, 0x0c, 0x3f);
    register(&mut vga, 0x0d, 0xff);
    let last = vga.screen();
    assert_eq!(last.start_address, 0x3fff);
    assert_eq!(
        last.cells[0],
        TextCell {
            character: b'Z',
            attribute: 0x3e
        }
    );
    assert_eq!(
        last.cells[1],
        TextCell {
            character: 0,
            attribute: 0
        }
    );

    // Scanout wraps at the 16-bit CRTC cell address, not the CPU aperture edge.
    register(&mut vga, 0x0c, 0xff);
    register(&mut vga, 0x0d, 0xff);
    let wrapped = vga.screen();
    assert_eq!(
        wrapped.cells[0],
        TextCell {
            character: 0,
            attribute: 0
        }
    );
    assert_eq!(wrapped.cells[1], first.cells[0]);
    vga.write_mmio(TEXT_MEMORY_START, 1, u64::from(b'X'));
    assert_eq!(vga.screen().cells[1].character, b'X');
    assert_eq!(wrapped.cells[1].character, b'A');
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn crtc_byte_and_paired_writes_share_register_state() {
    let mut vga = Vga::new();
    register(&mut vga, 0x0c, 0x12);
    vga.write_port(CRTC_INDEX_PORT, 1, 0x0d);
    vga.write_port(CRTC_DATA_PORT, 1, 0x34);
    register(&mut vga, 0x0e, 0xab);
    register(&mut vga, 0x0f, 0xcd);
    register(&mut vga, 0x0a, 0xe6);
    register(&mut vga, 0x0b, 0xe7);
    let screen = vga.screen();
    assert_eq!(screen.start_address, 0x1234);
    assert_eq!(screen.cursor.address, 0xabcd);
    assert_eq!(screen.cursor.start_scan_line, 0x26);
    assert_eq!(screen.cursor.end_scan_line, 0x67);
    assert_eq!(vga.read_port(CRTC_INDEX_PORT, 2), 0x670b);
    vga.write_port(CRTC_INDEX_PORT, 1, 0x2c); // Reserved index bits are ignored.
    assert_eq!(vga.read_port(CRTC_DATA_PORT, 1), 0x12);
    vga.write_port(CRTC_INDEX_PORT, 1, 0x1f);
    vga.write_port(CRTC_DATA_PORT, 1, 0x55);
    assert_eq!(vga.read_port(CRTC_DATA_PORT, 1), 0xff);
    assert_eq!(vga.screen(), screen);
}
