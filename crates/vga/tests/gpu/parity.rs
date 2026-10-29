use super::software;
use super::{capture, common, pixel, read_pixels};
use relocade_machine::Device;
use relocade_renderer::{fit_image, Image, Rect, Renderer};
use relocade_vga::{
    registers::{crtc, sequencer, COLOR_INPUT_STATUS_1_PORT, SEQUENCER_INDEX_PORT},
    CapturedFrame, TextCell, TextCursor, Vga, VgaRenderer, TEXT_MEMORY_START,
};
use std::{rc::Rc, sync::Arc};

async fn compare(
    renderer: &mut VgaRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    capture: Arc<CapturedFrame>,
) -> Vec<u8> {
    let software = software::rasterize(&capture);
    let mut encoder = device.create_command_encoder(&Default::default());
    let frame = renderer.encode(queue, &mut encoder, &capture);
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(device, queue, frame.image.texture()).await;
    assert_eq!(pixels.len(), software.pixels.len());
    for (index, (actual, expected)) in pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(software.pixels.as_chunks::<4>().0)
        .enumerate()
    {
        assert_eq!(actual, expected, "pixel ({}, {})", index % 640, index / 640);
    }
    pixels
}

async fn check_parity() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device, &queue).unwrap();
    for blink_on in [true, false] {
        let mut input = capture();
        input.blink_on = blink_on;
        for value in 0..256 {
            input.screen.cells[value] = TextCell {
                character: value as u8,
                attribute: 0x6f,
            };
            input.screen.cells[256 + value] = TextCell {
                character: b'A',
                attribute: value as u8,
            };
        }
        input.screen.cells[600] = TextCell {
            character: 0xdb,
            attribute: 0x94, // Blinking red on blue.
        };
        input.screen.cells[601] = TextCell {
            character: 0xdb,
            attribute: 0x14, // Steady red on blue.
        };
        let pixels = compare(&mut renderer, &device, &queue, Arc::new(input)).await;
        assert_eq!(
            pixel(&pixels, 320, 56),
            if blink_on {
                [170, 0, 0, 255]
            } else {
                [0, 0, 170, 255]
            }
        );
        assert_eq!(pixel(&pixels, 328, 56), [170, 0, 0, 255]);
    }

    for (display_start, address, start_row, end_row, lit_pixel) in [
        (0, 82, 6, 7, Some((16, 14))),       // Two-row underline.
        (0xffff, 0, 6, 7, Some((8, 6))),     // Address wrap.
        (0xffff, 0, 6, 0x27, Some((16, 6))), // One-cell skew after wrap.
        (0, 79, 6, 0x27, None),              // Skew goes into horizontal blanking.
        (0, 82, 0x26, 7, None),              // Disabled.
        (0, 82, 7, 6, None),                 // Reversed shape.
        (0, 82, 8, 31, None),                // Below the glyph.
        (0, 2000, 6, 7, None),               // Below the screen.
    ] {
        let mut input = capture();
        input.screen.start_address = display_start;
        input.screen.cursor = TextCursor {
            address,
            start_scan_line: start_row,
            end_scan_line: end_row,
        };
        let pixels = compare(&mut renderer, &device, &queue, Arc::new(input)).await;
        if let Some((x, y)) = lit_pixel {
            assert_eq!(pixel(&pixels, x, y), [170, 170, 170, 255]);
            assert_eq!(pixel(&pixels, x, y - 1), [0, 0, 0, 255]);
        } else {
            assert!(pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [0, 0, 0, 255]));
        }
    }
    let mut cursor_off = capture();
    cursor_off.blink_on = false;
    cursor_off.screen.cursor.start_scan_line = 6;
    let pixels = compare(&mut renderer, &device, &queue, Arc::new(cursor_off)).await;
    assert!(pixels
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [0, 0, 0, 255]));

    // Use real device writes for page selection and blanking, then retain both
    // captures after the source closes. Neither renderer may consult live VGA.
    let clock = Rc::new(common::ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    let source = vga.frame_source();
    vga.write_mmio(TEXT_MEMORY_START, 2, 0x04db); // Hidden page: red block.
    vga.write_mmio(TEXT_MEMORY_START + 0x200, 2, 0x0edb); // Visible page: yellow.
    common::crtc(&mut vga, crtc::START_ADDRESS_HIGH, 1);
    common::crtc(&mut vga, crtc::CURSOR_START, 0x20);
    clock.set(common::boundary(1));
    vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    let page = source.current_capture().unwrap();
    vga.write_port(
        SEQUENCER_INDEX_PORT,
        2,
        u32::from(u16::from_le_bytes([
            sequencer::CLOCKING_MODE,
            1 << 5, // Screen off.
        ])),
    );
    clock.set(common::boundary(2));
    vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    let blanked = source.current_capture().unwrap();
    drop(vga);
    assert!(source.next_capture(Some(blanked.sequence)).await.is_none());
    let pixels = compare(&mut renderer, &device, &queue, blanked).await;
    assert!(pixels
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [0, 0, 0, 255]));
    let pixels = compare(&mut renderer, &device, &queue, page.clone()).await;
    assert_eq!(pixel(&pixels, 0, 0), [255, 255, 85, 255]);

    // The decoder can be destroyed: different attachment sizes still use its
    // retained GPU image. Resizing cannot request a second VGA decode.
    let mut encoder = device.create_command_encoder(&Default::default());
    let image = renderer.encode(&queue, &mut encoder, &page).image;
    queue.submit([encoder.finish()]);
    drop(renderer);
    let mut drawing = Renderer::new(device.clone(), Image::FORMAT).unwrap();
    for size in [[640, 480], [320, 240]] {
        let output = Image::new(&device, size, (1, 1)).unwrap();
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output.view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            drawing.draw_image(&mut pass, &image, fit_image(&image, Rect::from_size(size)));
        }
        queue.submit([encoder.finish()]);
        let pixels = read_pixels(&device, &queue, output.texture()).await;
        assert_eq!(&pixels[..4], [255, 255, 85, 255]);
        assert_eq!(&pixels[pixels.len() - 4..], [0, 0, 0, 255]);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn matches_software_for_text_state_and_reuses_the_image_after_resize() {
    pollster::block_on(check_parity());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn matches_software_for_text_state_and_reuses_the_image_after_resize() {
    check_parity().await;
}
