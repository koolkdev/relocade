use super::software;
use super::{capture, common, pixel, read_pixels, set_cell, text_device};
use relocade_machine::Device;
use relocade_renderer::{fit_image, Image, Rect, Renderer};
use relocade_vga::{
    registers::{crtc, sequencer, COLOR_INPUT_STATUS_1_PORT, SEQUENCER_INDEX_PORT},
    CapturedFrame, VgaRenderer, TEXT_MEMORY_START,
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
    renderer.upload(queue, &capture);
    let frame = renderer.encode(&mut encoder).unwrap();
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
    let mut renderer = VgaRenderer::new(&device).unwrap();
    for blink_on in [true, false] {
        let mut vga = text_device();
        for value in 0..256 {
            set_cell(&mut vga, value, value as u8, 0x6f);
            set_cell(&mut vga, 256 + value, b'A', value as u8);
        }
        set_cell(&mut vga, 600, 0xdb, 0x94);
        set_cell(&mut vga, 601, 0xdb, 0x14);
        let mut input = common::fixture::capture(&vga);
        input.display.blink_on = blink_on;
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
        let [high, low] = (display_start as u16).to_be_bytes();
        input.display.registers.crtc.values[12] = high;
        input.display.registers.crtc.values[13] = low;
        let [high, low] = (address as u16).to_be_bytes();
        input.display.registers.crtc.values[14] = high;
        input.display.registers.crtc.values[15] = low;
        input.display.registers.crtc.values[10] = start_row;
        input.display.registers.crtc.values[11] = end_row;
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
    cursor_off.display.blink_on = false;
    cursor_off.display.registers.crtc.values[10] = 6;
    let pixels = compare(&mut renderer, &device, &queue, Arc::new(cursor_off)).await;
    assert!(pixels
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [0, 0, 0, 255]));

    // Use real device writes for page selection and blanking, then retain both
    // captures after the source closes. Neither renderer may consult live VGA.
    let clock = Rc::new(common::ManualClock::default());
    let mut vga = common::text_vga(clock.clone());
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
    renderer.upload(&queue, &page);
    let image = renderer.encode(&mut encoder).unwrap().image;
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

#[path = "../support/programmed_text.rs"]
mod programmed;

async fn check_programmed_text() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    let cases = programmed::cases();
    // Revisit retained snapshots after other sizes/fonts/palettes have been rendered.
    for case in cases.iter().chain(cases.iter().rev()) {
        let capture = CapturedFrame {
            display: case.display.clone(),
            time: std::time::Duration::ZERO,
            sequence: 1,
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.upload(&queue, &capture);
        let frame = renderer.encode(&mut encoder).unwrap();
        queue.submit([encoder.finish()]);
        assert_eq!(frame.image.size(), case.size, "{}", case.name);
        let pixels = read_pixels(&device, &queue, frame.image.texture()).await;
        assert_eq!(
            pixels,
            software::render(&capture.display).as_ref(),
            "{}",
            case.name
        );
        for &(x, y, expected) in &case.pixels {
            let offset = ((y * case.size[0] + x) * 4) as usize;
            assert_eq!(
                pixels[offset..offset + 4],
                expected,
                "{} ({x},{y})",
                case.name
            );
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn programmable_text_and_retained_snapshots_match_literal_pixels() {
    pollster::block_on(check_programmed_text());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn programmable_text_and_retained_snapshots_match_literal_pixels() {
    check_programmed_text().await;
}

async fn check_published_updates() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    let clock = Rc::new(common::ManualClock::default());
    let mut vga = common::text_vga(clock.clone());
    let source = vga.frame_source();
    common::crtc(&mut vga, crtc::CURSOR_START, 0x20);
    set_cell(&mut vga, 0, 0xdb, 4);
    clock.set(common::boundary(1));
    vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    let first = source.current_capture().unwrap();
    let pixels = compare(&mut renderer, &device, &queue, first.clone()).await;
    assert_eq!(pixel(&pixels, 0, 0), [170, 0, 0, 255]);

    set_cell(&mut vga, 0, 0xdb, 1);
    clock.set(common::boundary(2));
    vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    let pixels = compare(
        &mut renderer,
        &device,
        &queue,
        source.current_capture().unwrap(),
    )
    .await;
    assert_eq!(pixel(&pixels, 0, 0), [0, 0, 170, 255]);

    for (port, value) in [
        (0x3c4, 0x0402),
        (0x3c4, 0x0604),
        (0x3ce, 0x0005),
        (0x3ce, 0x0506),
    ] {
        vga.write_port(port, 2, value);
    }
    vga.write_mmio(0xa0000 + 0xdb * 32, 1, 0); // Clear the block glyph's first row.
    for (port, value) in [
        (0x3c4, 0x0302),
        (0x3c4, 0x0204),
        (0x3ce, 0x1005),
        (0x3ce, 0x0e06),
    ] {
        vga.write_port(port, 2, value);
    }
    clock.set(common::boundary(3));
    vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1); // The renderer skips this font change.
    set_cell(&mut vga, 1, 0xdb, 2);
    clock.set(common::boundary(4));
    vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    let newest = source.current_capture().unwrap();
    drop(vga);
    let pixels = compare(&mut renderer, &device, &queue, newest).await;
    assert_eq!(pixel(&pixels, 0, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 0, 1), [0, 0, 170, 255]);
    assert_eq!(pixel(&pixels, 8, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 8, 1), [0, 170, 0, 255]);
    let pixels = compare(&mut renderer, &device, &queue, first).await;
    assert_eq!(pixel(&pixels, 0, 0), [170, 0, 0, 255]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn published_text_and_font_updates_match_pixels_across_skipped_captures() {
    pollster::block_on(check_published_updates());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn published_text_and_font_updates_match_pixels_across_skipped_captures() {
    check_published_updates().await;
}
