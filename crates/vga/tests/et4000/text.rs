use super::*;
use relocade_vga::{DisplaySnapshot, FramebufferGeometry};

fn text_card(clock: &ManualClock) -> ConnectedEt4000 {
    let device = ConnectedEt4000::new(clock);
    let io = &device.io;
    io.write_port(0x3bf, 1, 3);
    io.write_port(0x3d8, 1, 0xa0);
    // Load two distinct fonts into plane two, independently of the text page.
    for value in [0x0101, 0x0402, 0x2103, 0x0604] {
        io.write_port(0x3c4, 2, value);
    }
    io.write_port(0x3ce, 2, 0x0005);
    io.write_port(0x3ce, 2, 0x0406);
    io.write_mmio(0xa2820, 4, 0x0000_4080); // Font A, character A.
    io.write_mmio(0xa4820, 4, 0x0000_1020); // Font B, character A.
    for value in [0x0302, 0x0204] {
        io.write_port(0x3c4, 2, value);
    }
    io.write_port(0x3ce, 2, 0x1005);
    io.write_port(0x3ce, 2, 0x0e06);
    io.write_mmio(0xb8000, 2, 0x0242); // Distinguish the low page.
    io.write_port(0x3cd, 1, 2);
    io.write_mmio(0xb8000, 4, 0x0241_0941);
    io.write_mmio(0xb8004, 2, 0x0100); // Blank glyph under the skewed cursor.

    io.write_port(0x3d4, 2, 0x0011);
    for (index, value) in [
        (1, 3),
        (2, 4),
        (6, 15),
        (7, 0x10),
        (9, 0x43),
        (0x0a, 2),
        (0x0b, 0x22),
        (0x0f, 1),
        (0x10, 12),
        (0x11, 14),
        (0x12, 7),
        (0x13, 2),
        (0x15, 8),
        (0x16, 16),
        (0x17, 0xa3),
        (0x33, 5),
    ] {
        support::registers::indexed(io, 0x3d4, index, value);
    }
    support::registers::attribute(io, 0x10, 0);
    support::registers::attribute(io, 0x13, 0);
    support::registers::palette(io, 1, [63, 0, 0]);
    support::registers::palette(io, 2, [0, 63, 0]);
    device
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn extended_text_page_restores_pending_start_and_retains_drawing_locations() {
    let clock = ManualClock::default();
    let device = text_card(&clock);
    let io = &device.io;
    io.write_port(0x3cd, 1, 0xf7);
    let cell = {
        let mut card = device.card.borrow_mut();
        let framebuffer = card.framebuffer();
        assert_eq!(
            framebuffer.geometry(),
            Some(FramebufferGeometry::Text {
                columns: 4,
                rows: 2
            })
        );
        assert_eq!(framebuffer.text_cell(0, 0).unwrap().value(), 0x0941);
        assert_eq!(framebuffer.text_cell(0, 1).unwrap().value(), 0x0241);
        framebuffer.text_cell(0, 0).unwrap()
    };
    assert_eq!(
        device.card.borrow().snapshot().vga().unwrap().display_start,
        0
    );
    clock.advance_to(clock.at(Duration::from_millis(1)));
    io.read_port(0x3da, 1);
    let retained = device.card.borrow().snapshot();
    assert_eq!(retained.vga().unwrap().display_start, 0x10000);
    assert_eq!(retained.vga().unwrap().layout.cursor_address, 0x10001);

    // The next high start is pending. In IBM word mapping, counter bit 17
    // shifts beyond installed DRAM, so this page wraps to physical byte zero.
    io.write_port(0x3d4, 2, 0x0633);
    let (core, extended) = {
        let card = device.card.borrow();
        (card.core_state(), card.et4000_state())
    };
    assert_eq!(
        device
            .card
            .borrow_mut()
            .framebuffer()
            .text_cell(0, 0)
            .unwrap()
            .value(),
        0x0242
    );
    io.write_port(0x3d4, 2, 0x0033);
    io.with_context(|context| {
        device.card.borrow_mut().restore_state(
            context,
            Some(core.clone()),
            None,
            Some(extended.clone()),
        );
    });
    {
        let mut card = device.card.borrow_mut();
        assert_eq!(card.core_state(), core);
        assert_eq!(card.et4000_state(), extended);
        assert_eq!(card.snapshot().vga().unwrap().display_start, 0x10000);
        card.framebuffer().write_text_cell(&cell, 0x0943);
        assert_eq!(card.framebuffer().text_cell(0, 0).unwrap().value(), 0x0242);
        assert_eq!(card.snapshot().vram_byte(0x80000), Some(b'C'));
    }
    assert_eq!(retained.vram_byte(0x80000), Some(b'A'));
    clock.advance_to(clock.at(Duration::from_millis(2)));
    io.read_port(0x3da, 1);
    let snapshot = device.card.borrow().snapshot();
    assert_eq!(snapshot.vga().unwrap().display_start, 0x20000);
    assert_eq!(snapshot.vga().unwrap().layout.cursor_address, 0x10001);
}

fn text_captures() -> [DisplaySnapshot; 3] {
    let clock = ManualClock::default();
    let device = text_card(&clock);
    let io = &device.io;
    clock.advance_to(clock.at(Duration::from_millis(1)));
    io.read_port(0x3da, 1);
    let matching = device.card.borrow().snapshot();
    assert!(matching.vga().unwrap().cursor_blink_on);
    io.write_port(0x3d4, 2, 0x0133); // Same low cursor, different high bits.
    let hidden = device.card.borrow().snapshot();
    assert_eq!(hidden.vga().unwrap().layout.cursor_address, 1);

    // Tseng mapping uses consecutive plane groups even with word mode selected.
    // Draw through the native view, then inspect independent physical addresses.
    for value in [0x6036, 0x0333, 0xff0c, 0xff0d, 0x000f] {
        io.write_port(0x3d4, 2, value);
    }
    {
        let mut card = device.card.borrow_mut();
        let mut framebuffer = card.framebuffer();
        for (column, value) in [(0, 0x0941), (1, 0x0241), (2, 0x0100)] {
            let cell = framebuffer.text_cell(0, column).unwrap();
            framebuffer.write_text_cell(&cell, value);
        }
    }
    clock.advance_to(clock.at(Duration::from_millis(2)));
    io.read_port(0x3da, 1);
    let wrapped = device.card.borrow().snapshot();
    assert_eq!(wrapped.vga().unwrap().display_start, 0x3ffff);
    assert_eq!(wrapped.vram_byte(0xffffc), Some(b'A'));
    assert_eq!(wrapped.vram_byte(0xffffd), Some(9));
    assert_eq!(wrapped.vram_byte(0), Some(b'A'));
    assert_eq!(wrapped.vram_byte(1), Some(2));
    assert_eq!(wrapped.vram_byte(0x2820 * 4 + 2), Some(0x80));
    assert_eq!(wrapped.vram_byte(0x4820 * 4 + 2), Some(0x20));
    [matching, hidden, wrapped]
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn high_cursor_bits_update_immediately_and_contiguous_text_wraps_without_moving_fonts() {
    text_captures();
}

#[cfg(feature = "renderer")]
mod gpu {
    use super::*;
    use relocade_vga::VgaRenderer;

    async fn check() {
        let [matching, hidden, wrapped] = text_captures();
        let adapter = wgpu::Instance::default()
            .request_adapter(&Default::default())
            .await
            .unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let mut renderer = VgaRenderer::new(&device).unwrap();
        for (snapshot, cursor) in [
            (matching.clone(), true),
            (hidden, false),
            (wrapped, true),
            (matching, true),
        ] {
            let mut encoder = device.create_command_encoder(&Default::default());
            renderer.upload(&queue, &snapshot);
            let frame = renderer.encode(&mut encoder).unwrap();
            assert_eq!(frame.size(), [32, 8]);
            queue.submit([encoder.finish()]);
            let pixels = readback::read_pixels(&device, &queue, frame.texture()).await;
            for (x, y, expected) in [
                (0, 0, [255, 0, 0, 255]),  // Attribute bit three selects font A.
                (10, 0, [0, 255, 0, 255]), // Font B has a different dot pattern.
                (8, 0, [0, 0, 0, 255]),
                (8, 2, [0, 0, 0, 255]), // Cursor skew leaves its addressed cell blank.
                (
                    16,
                    2,
                    if cursor {
                        [255, 0, 0, 255]
                    } else {
                        [0, 0, 0, 255]
                    },
                ),
                (
                    23,
                    2,
                    if cursor {
                        [255, 0, 0, 255]
                    } else {
                        [0, 0, 0, 255]
                    },
                ),
            ] {
                let offset = (y * 32 + x) * 4;
                assert_eq!(&pixels[offset..offset + 4], &expected, "({x}, {y})");
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn extended_text_fonts_cursor_and_wrap_render_from_retained_captures() {
        pollster::block_on(check());
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test::wasm_bindgen_test]
    async fn extended_text_fonts_cursor_and_wrap_render_from_retained_captures() {
        check().await;
    }
}
