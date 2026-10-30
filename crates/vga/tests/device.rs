mod common;
use common::ManualClock;
use relocade_machine::Device;
use relocade_vga::{Vga, CRTC_DATA_PORT, CRTC_INDEX_PORT, TEXT_MEMORY_START};
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn register(vga: &mut impl Device, index: u8, value: u8) {
    vga.write_port(
        CRTC_INDEX_PORT,
        2,
        u32::from(u16::from_le_bytes([index, value])),
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn text_aperture_selects_planes_and_scanout_uses_full_crtc_addresses() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    vga.write_mmio(TEXT_MEMORY_START, 4, 0x2f42_1e41);
    vga.write_mmio(TEXT_MEMORY_START + 1, 1, 0x17);
    assert_eq!(vga.read_mmio(TEXT_MEMORY_START, 4), 0x2f42_1741);
    let first = common::support::inspection::inspect(&vga.snapshot());
    assert_eq!(
        first.cells[0],
        common::support::inspection::Cell {
            character: b'A',
            attribute: 0x17
        }
    );
    assert_eq!(
        first.cells[1],
        common::support::inspection::Cell {
            character: b'B',
            attribute: 0x2f
        }
    );

    // The final CPU-accessible cell precedes video memory outside that aperture.
    vga.write_mmio(TEXT_MEMORY_START + 0x7ffe, 2, 0x3e5a);
    register(&mut vga, 0x0c, 0x3f);
    register(&mut vga, 0x0d, 0xff);
    let last = common::support::inspection::inspect(&vga.snapshot());
    assert_eq!(last.start_address, 0x3fff);
    assert_eq!(
        last.cells[0],
        common::support::inspection::Cell {
            character: b'Z',
            attribute: 0x3e
        }
    );
    assert_eq!(
        last.cells[1],
        common::support::inspection::Cell {
            character: 0,
            attribute: 0
        }
    );

    // Scanout wraps at the 16-bit CRTC cell address, not the CPU aperture edge.
    register(&mut vga, 0x0c, 0xff);
    register(&mut vga, 0x0d, 0xff);
    let wrapped = common::support::inspection::inspect(&vga.snapshot());
    assert_eq!(
        wrapped.cells[0],
        common::support::inspection::Cell {
            character: 0,
            attribute: 0
        }
    );
    assert_eq!(wrapped.cells[1], first.cells[0]);
    vga.write_mmio(TEXT_MEMORY_START, 1, u64::from(b'X'));
    assert_eq!(
        common::support::inspection::inspect(&vga.snapshot()).cells[1].character,
        b'X'
    );
    assert_eq!(wrapped.cells[1].character, b'A');
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn crtc_byte_and_paired_writes_share_register_state() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    register(&mut vga, 0x0c, 0x12);
    vga.write_port(CRTC_INDEX_PORT, 1, 0x0d);
    vga.write_port(CRTC_DATA_PORT, 1, 0x34);
    register(&mut vga, 0x0e, 0xab);
    register(&mut vga, 0x0f, 0xcd);
    register(&mut vga, 0x0a, 0xe6);
    register(&mut vga, 0x0b, 0xe7);
    let screen = common::support::inspection::inspect(&vga.snapshot());
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
    assert_eq!(
        common::support::inspection::inspect(&vga.snapshot()),
        screen
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn protected_registers_and_monochrome_port_selection_match_guest_reads() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    register(&mut vga, 0, 0x10);
    assert_eq!(vga.read_port(0x3d5, 1), 0x5f);
    register(&mut vga, 7, 0xe0);
    assert_eq!(vga.read_port(0x3d5, 1), 0x0f); // Only line-compare bit 8 changed.
    register(&mut vga, 0x11, 0x2e);
    register(&mut vga, 0, 0x10);
    assert_eq!(vga.read_port(0x3d4, 2), 0x1000);
    vga.write_port(0x3c2, 1, 0x42); // Monochrome CRTC and status addresses.
    assert_eq!(vga.read_port(0x3cc, 1), 0x42);
    assert_eq!(vga.read_port(0x3d4, 2), 0xffff);
    assert_eq!(vga.read_port(0x3da, 1), 0xff);
    vga.write_port(0x3b4, 2, 0x0a00);
    assert_eq!(vga.read_port(0x3b4, 2), 0x0a00);
    assert_eq!(vga.read_port(0x3ba, 1), 0);
    vga.write_mmio(TEXT_MEMORY_START, 2, 0x0741);
    vga.write_port(0x3c2, 1, 0x40); // Disable display RAM decoding.
    vga.write_mmio(TEXT_MEMORY_START, 2, 0x075a);
    assert_eq!(vga.read_mmio(TEXT_MEMORY_START, 2), 0xffff);
    vga.write_port(0x3c2, 1, 0x42);
    assert_eq!(vga.read_mmio(TEXT_MEMORY_START, 2), 0x0741);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sequencer_readback_preserves_defined_fields_and_selects_supported_registers() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    // Clocking-mode bits 7, 6 and 1 have undefined readback (IBM VGA p2-49).
    for (index, written, mask, expected) in [
        (0, 0xfc, 0x03, 0),
        (1, 0xff, 0x3d, 0x3d),
        (2, 0xaa, 0x0f, 0x0a),
        (3, 0xff, 0x3f, 0x3f),
        (4, 0xff, 0x0e, 0x0e),
    ] {
        vga.write_port(0x3c4, 2, u32::from(u16::from_le_bytes([index, written])));
        assert_eq!(vga.read_port(0x3c5, 1) & mask, expected);
    }
    vga.write_port(0x3c4, 1, 0xff);
    assert_eq!(vga.read_port(0x3c4, 1) & 7, 7);
    assert_eq!(vga.read_port(0x3c5, 1), 0xff);
}
