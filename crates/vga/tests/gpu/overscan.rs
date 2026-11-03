//! Whole-image expectations: border placement and color do not use active-picture readback.
use super::{common, graphics::Gpu};
use common::support::{
    indexed256, planar16,
    registers::{attribute, indexed, palette},
};
use relocade_machine::Device;
use relocade_vga::Vga;
use std::{ops::Range, rc::Rc};

const WHITE: [u8; 4] = [255; 4];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

fn device(layout: u8) -> Vga<common::ManualClock> {
    let mut vga = Vga::with_clock(Rc::new(common::ManualClock::default()));
    match layout {
        0 | 1 => indexed(&mut vga, 0x3c4, 1, layout),
        2 => planar16::program(&mut vga),
        3 => indexed256::program(&mut vga),
        _ => unreachable!(),
    }
    // Ten clocks by ten lines. Display enable covers two clocks by four lines;
    // blanking leaves two border clocks/lines on each side of that picture.
    for (index, value) in [
        (0x11, 0),
        (0, 5),
        (1, 1),
        (2, 3),
        (3, 0x87),
        (5, 0),
        (6, 8),
        (7, 0),
        (8, 0),
        (9, 0),
        (10, 0x20),
        (0x12, 3),
        (0x15, 5),
        (0x16, 7),
        (0x17, 0xa3),
        (0x18, 255),
    ] {
        common::crtc(&mut vga, index, value);
    }
    attribute(&mut vga, 0, 0);
    attribute(&mut vga, 0x11, 0xb2);
    palette(&mut vga, 0, [63; 3]);
    palette(&mut vga, 0xb2, [63, 0, 0]);
    vga
}

fn picture(size: [u32; 2], x: Range<u32>, y: Range<u32>, border: [u8; 4]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((size[0] * size[1] * 4) as usize);
    for row in 0..size[1] {
        for column in 0..size[0] {
            pixels.extend_from_slice(if x.contains(&column) && y.contains(&row) {
                &WHITE
            } else {
                &border
            });
        }
    }
    pixels
}

async fn check_layouts() {
    let mut gpu = Gpu::new().await;
    for (layout, width, active) in [
        (0, 54, 18..36),
        (1, 48, 16..32),
        (2, 48, 16..32),
        (3, 24, 8..16),
    ] {
        let mut vga = device(layout);
        let earlier = common::fixture::capture(&vga);
        let expected = picture([width, 8], active.clone(), 2..6, RED);
        gpu.expect(&earlier, [width, 8], &expected).await;

        // Palette-only changes redraw the border without changing its geometry.
        palette(&mut vga, 0xb2, [0, 63, 0]);
        gpu.expect(
            &common::fixture::capture(&vga),
            [width, 8],
            &picture([width, 8], active, 2..6, GREEN),
        )
        .await;
        drop(vga);
        gpu.expect(&earlier, [width, 8], &expected).await;
    }
}

async fn check_palette_and_gates() {
    let mut gpu = Gpu::new().await;
    let mut vga = device(2);
    // Ordinary colors follow the attribute palette, plane enable and Color
    // Select. Overscan uses the complete AC11 index, bypassing all three.
    attribute(&mut vga, 0, 3);
    attribute(&mut vga, 0x10, 0x81);
    attribute(&mut vga, 0x12, 0);
    attribute(&mut vga, 0x14, 15);
    palette(&mut vga, 0xf3, [63; 3]);
    gpu.expect(
        &common::fixture::capture(&vga),
        [48, 8],
        &picture([48, 8], 16..32, 2..6, RED),
    )
    .await;

    palette(&mut vga, 3, [63; 3]);
    palette(&mut vga, 2, [0, 63, 0]);
    vga.write_port(0x3c6, 1, 15);
    gpu.expect(
        &common::fixture::capture(&vga),
        [48, 8],
        &picture([48, 8], 16..32, 2..6, GREEN),
    )
    .await;

    // The divided-clock border convention suppresses border color, not geometry.
    indexed(&mut vga, 0x3c4, 1, 9);
    gpu.expect(
        &common::fixture::capture(&vga),
        [48, 8],
        &picture([48, 8], 16..32, 2..6, BLACK),
    )
    .await;
    indexed(&mut vga, 0x3c4, 1, 0x21); // Screen off covers picture and border.
    gpu.expect(
        &common::fixture::capture(&vga),
        [48, 8],
        &BLACK.repeat(48 * 8),
    )
    .await;
    indexed(&mut vga, 0x3c4, 1, 1);
    vga.read_port(0x3da, 1);
    vga.write_port(0x3c0, 1, 0); // Attribute display disabled.
    gpu.expect(
        &common::fixture::capture(&vga),
        [48, 8],
        &BLACK.repeat(48 * 8),
    )
    .await;
}

async fn check_crop() {
    let mut gpu = Gpu::new().await;
    let mut vga = device(2);
    common::crtc(&mut vga, 3, 0xa7); // One clock of display-enable skew.
    gpu.expect(
        &common::fixture::capture(&vga),
        [48, 8],
        &picture([48, 8], 24..40, 2..6, RED),
    )
    .await;

    // Interior blanking removes a clock of active video. The nonblank interval
    // begins at clock two and wraps through zero; it contains no blank stripe.
    common::crtc(&mut vga, 2, 0);
    common::crtc(&mut vga, 3, 0x81);
    gpu.expect(
        &common::fixture::capture(&vga),
        [72, 8],
        &picture([72, 8], 64..72, 2..6, RED),
    )
    .await;

    common::crtc(&mut vga, 2, 255);
    common::crtc(&mut vga, 0x15, 255);
    gpu.expect(
        &common::fixture::capture(&vga),
        [80, 10],
        &picture([80, 10], 0..16, 0..4, RED),
    )
    .await;
    common::crtc(&mut vga, 2, 3);
    common::crtc(&mut vga, 3, 0x83); // No end match before the next start.
    gpu.expect(&common::fixture::capture(&vga), [1, 1], &BLACK)
        .await;
}

async fn check_repeats() {
    let mut gpu = Gpu::new().await;
    let mut vga = device(3);
    common::crtc(&mut vga, 9, 1); // Two scanlines per graphics memory row.
    gpu.expect(
        &common::fixture::capture(&vga),
        [24, 8],
        &picture([24, 8], 8..16, 2..6, RED),
    )
    .await;

    common::crtc(&mut vga, 0x15, 4); // One bottom border line, half a pair.
    gpu.expect(
        &common::fixture::capture(&vga),
        [24, 7],
        &picture([24, 7], 8..16, 2..6, RED),
    )
    .await;
    common::crtc(&mut vga, 0x17, 0xa7); // The vertical counter now ticks every other line.
    gpu.expect(
        &common::fixture::capture(&vga),
        [24, 14],
        &picture([24, 14], 8..16, 4..12, RED),
    )
    .await;
    common::crtc(&mut vga, 0x17, 0xa3);
    common::crtc(&mut vga, 0x16, 6); // Move the crop origin to physical line seven.
    common::crtc(&mut vga, 0x13, 1); // Eight pixels per memory row.
    palette(&mut vga, 1, [0, 63, 0]);
    for offset in 8..16 {
        vga.write_mmio(0xa0000 + offset, 1, 1);
    }
    let mut expected = picture([24, 8], 8..16, 3..7, RED);
    // The second memory row occupies two distinct physical rows in the image,
    // even when the crop begins on an odd scanline.
    for row in 5..7 {
        let start = (row * 24 + 8) * 4;
        expected[start..start + 8 * 4].copy_from_slice(&GREEN.repeat(8));
    }
    gpu.expect(&common::fixture::capture(&vga), [24, 8], &expected)
        .await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn overscan_surrounds_text_and_graphics_and_survives_retained_captures() {
    pollster::block_on(check_layouts());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn overscan_surrounds_text_and_graphics_and_survives_retained_captures() {
    check_layouts().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn overscan_uses_its_dac_index_and_obeys_output_gates() {
    pollster::block_on(check_palette_and_gates());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn overscan_uses_its_dac_index_and_obeys_output_gates() {
    check_palette_and_gates().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn crop_and_display_skew_have_independent_origins() {
    pollster::block_on(check_crop());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn crop_and_display_skew_have_independent_origins() {
    check_crop().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn border_edges_preserve_physical_scanlines() {
    pollster::block_on(check_repeats());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn border_edges_preserve_physical_scanlines() {
    check_repeats().await;
}
