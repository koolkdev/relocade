use super::{
    common,
    graphics::{load_palette, pixels, Gpu},
};
use common::support::{
    planar16::{program, write_pixels},
    registers::{attribute, indexed},
};
use relocade_machine::Device;
use relocade_vga::Vga;
use std::rc::Rc;

async fn check_pixels() {
    let mut gpu = Gpu::new().await;
    let clock = Rc::new(common::ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    program(&mut vga);
    load_palette(&mut vga);
    let mut indices: Vec<u8> = (0..480)
        .flat_map(|y| (0..640).map(move |x| ((x ^ (x / 16) ^ (y * 3)) & 15) as u8))
        .collect();
    write_pixels(&mut vga, &indices);
    clock.set(common::dots(490 * 800, 25_175_000)); // 640×480 retrace.
    vga.read_port(0x3da, 1);
    let source = vga.frame_source();
    let first = source.current_capture().unwrap();
    let original = pixels(indices.iter().copied());
    gpu.expect(&first, [640, 480], &original).await;

    // Write mode two updates one pixel through CPU latches and a bit mask.
    // This uses the same published dirty-page path as text and eight-bit pixels.
    indexed(&mut vga, 0x3ce, 5, 2);
    indexed(&mut vga, 0x3ce, 8, 0x20);
    vga.read_mmio(0xa0000 + 12345, 1);
    vga.write_mmio(0xa0000 + 12345, 1, 3);
    indices[12345 * 8 + 2] = 3;
    clock.set(common::dots((490 + 525) * 800, 25_175_000));
    vga.read_port(0x3da, 1);
    gpu.expect(
        &source.current_capture().unwrap(),
        [640, 480],
        &pixels(indices.iter().copied()),
    )
    .await;

    // All four-bit colors share attribute mapping, plane enable, Color Select
    // replacement and the final DAC mask with text. No VRAM writes are needed.
    for color in 0..16 {
        attribute(&mut vga, color, (color * 3 + 17) & 63);
    }
    attribute(&mut vga, 0x12, 5);
    attribute(&mut vga, 0x10, 0x81);
    attribute(&mut vga, 0x14, 0x0d);
    vga.write_port(0x3c6, 1, 0x7f);
    let expected = pixels(
        indices
            .iter()
            .map(|c| [0x51, 0x54, 0x5d, 0x50][usize::from((c & 1) | ((c & 4) >> 1))]),
    );
    gpu.expect(&common::fixture::capture(&vga), [640, 480], &expected)
        .await;
    drop(vga);
    gpu.expect(&first, [640, 480], &original).await;
}

async fn check_addressing() {
    let mut gpu = Gpu::new().await;
    let mut vga = Vga::with_clock(Rc::new(common::ManualClock::default()));
    program(&mut vga);
    load_palette(&mut vga);
    // A 16-pixel window into rows with a 32-pixel stride; two scanlines per row.
    for (index, value) in [(1, 1), (7, 0), (9, 1), (13, 1), (0x12, 3), (0x13, 2)] {
        indexed(&mut vga, 0x3d4, index, value);
    }
    let indices: Vec<_> = (0..96).map(|x| ((x + x / 16) & 15) as u8).collect();
    write_pixels(&mut vga, &indices);
    let expected = |ranges: &[std::ops::Range<usize>]| {
        pixels(
            ranges
                .iter()
                .flat_map(|r| indices[r.clone()].iter().copied()),
        )
    };
    gpu.expect(
        &common::fixture::capture(&vga),
        [16, 2],
        &expected(&[8..24, 40..56]),
    )
    .await;
    attribute(&mut vga, 0x13, 7); // Cross an eight-pixel byte boundary.
    gpu.expect(
        &common::fixture::capture(&vga),
        [16, 2],
        &expected(&[15..31, 47..63]),
    )
    .await;
    attribute(&mut vga, 0x10, 0x21);
    indexed(&mut vga, 0x3d4, 0x18, 0); // Split the repeated pair and reset panning.
    gpu.expect(
        &common::fixture::capture(&vga),
        [16, 4],
        &expected(&[15..31, 0..16, 0..16, 32..48]),
    )
    .await;

    // Dividing the vertical counter doubles the display and split positions,
    // while the row-scan counter still advances once per physical scanline.
    indexed(&mut vga, 0x3d4, 0x17, 0xe7);
    gpu.expect(
        &common::fixture::capture(&vga),
        [16, 4],
        &expected(&[15..31, 0..16, 32..48, 64..80]),
    )
    .await;

    // Count-by-two repeats whole eight-pixel plane groups, not individual bits.
    attribute(&mut vga, 0x13, 0);
    indexed(&mut vga, 0x3d4, 0x18, 255);
    indexed(&mut vga, 0x3d4, 0x17, 0xeb);
    gpu.expect(
        &common::fixture::capture(&vga),
        [16, 2],
        &expected(&[8..16, 8..16, 40..48, 40..48]),
    )
    .await;

    // Other shifter/attribute combinations must not be mistaken for planar16.
    for (shift, mode) in [(0x20, 1), (0x40, 1), (0, 0x41)] {
        indexed(&mut vga, 0x3ce, 5, shift);
        attribute(&mut vga, 0x10, mode);
        gpu.expect(
            &common::fixture::capture(&vga),
            [16, 4],
            &[0, 0, 0, 255].repeat(64),
        )
        .await;
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn planar_pixels_masks_palette_and_retained_captures() {
    pollster::block_on(check_pixels());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn planar_pixels_masks_palette_and_retained_captures() {
    check_pixels().await;
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn planar_addressing_pan_split_count_and_layout_selection() {
    pollster::block_on(check_addressing());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn planar_addressing_pan_split_count_and_layout_selection() {
    check_addressing().await;
}
