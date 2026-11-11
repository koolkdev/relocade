use super::{
    common,
    graphics::{load_palette, pixels, Gpu},
};
use crate::common::fixture::ConnectedVga;
use common::support::{
    planar16::{program, write_pixels},
    registers::{attribute, indexed},
};
use relocade_clock_test_support::ManualClock;

async fn check_pixels() {
    let mut gpu = Gpu::new().await;
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(ConnectedVga::new(clock.clock()));
    program(&vga.io);
    load_palette(&vga.io);
    let mut indices: Vec<u8> = (0..480)
        .flat_map(|y| (0..640).map(move |x| ((x ^ (x / 16) ^ (y * 3)) & 15) as u8))
        .collect();
    write_pixels(&vga.io, &indices);
    clock.advance_to(clock.at(common::dots(490 * 800, 25_175_000))); // 640×480 retrace.
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    let source = vga.device.borrow().frames();
    let first = source.try_recv().unwrap();
    let original = pixels(indices.iter().copied());
    gpu.expect(&first, [640, 480], &original).await;

    // Write mode two updates one pixel through CPU latches and a bit mask.
    // This uses the same published dirty-page path as text and eight-bit pixels.
    indexed(&vga.io, 0x3ce, 5, 2);
    indexed(&vga.io, 0x3ce, 8, 0x20);
    vga.io.read_mmio(0xa0000 + 12345, 1);
    vga.io.write_mmio(0xa0000 + 12345, 1, 3);
    indices[12345 * 8 + 2] = 3;
    clock.advance_to(clock.at(common::dots((490 + 525) * 800, 25_175_000)));
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    gpu.expect(
        &source.try_recv().unwrap(),
        [640, 480],
        &pixels(indices.iter().copied()),
    )
    .await;

    // All four-bit colors share attribute mapping, plane enable, Color Select
    // replacement and the final DAC mask with text. No VRAM writes are needed.
    for color in 0..16 {
        attribute(&vga.io, color, (color * 3 + 17) & 63);
    }
    attribute(&vga.io, 0x12, 5);
    attribute(&vga.io, 0x10, 0x81);
    attribute(&vga.io, 0x14, 0x0d);
    vga.io.write_port(0x3c6, 1, 0x7f);
    let expected = pixels(
        indices
            .iter()
            .map(|c| [0x51, 0x54, 0x5d, 0x50][usize::from((c & 1) | ((c & 4) >> 1))]),
    );
    gpu.expect(&vga.snapshot(), [640, 480], &expected).await;
    drop(video);
    drop(vga);
    gpu.expect(&first, [640, 480], &original).await;
}

async fn check_addressing() {
    let mut gpu = Gpu::new().await;
    let clock = ManualClock::default();
    let vga = ConnectedVga::new(clock.clock());
    program(&vga.io);
    indexed(&vga.io, 0x3d4, 13, 1);
    clock.advance_to(clock.at(common::dots(490 * 800, 25_175_000)));
    vga.io.read_port(0x3da, 1); // Latch the start before changing the viewport.
    load_palette(&vga.io);
    // A 16-pixel window into rows with a 32-pixel stride; two scanlines per row.
    for (index, value) in [(1, 1), (7, 0), (9, 1), (13, 1), (0x12, 3), (0x13, 2)] {
        indexed(&vga.io, 0x3d4, index, value);
    }
    let indices: Vec<_> = (0..96).map(|x| ((x + x / 16) & 15) as u8).collect();
    write_pixels(&vga.io, &indices);
    let expected = |ranges: &[std::ops::Range<usize>]| {
        pixels(
            ranges
                .iter()
                .flat_map(|r| indices[r.clone()].iter().copied()),
        )
    };
    gpu.expect(&vga.snapshot(), [16, 2], &expected(&[8..24, 40..56]))
        .await;
    attribute(&vga.io, 0x13, 7); // Cross an eight-pixel byte boundary.
    gpu.expect(&vga.snapshot(), [16, 2], &expected(&[15..31, 47..63]))
        .await;
    indexed(&vga.io, 0x3d4, 8, 0x20); // One additional byte of horizontal panning.
    attribute(&vga.io, 0x10, 0x21);
    indexed(&vga.io, 0x3d4, 0x18, 0); // Split the repeated pair and reset panning.
    gpu.expect(
        &vga.snapshot(),
        [16, 4],
        &expected(&[23..39, 0..16, 0..16, 32..48]),
    )
    .await;
    attribute(&vga.io, 0x10, 1); // Preserve both panning shifts after the split.
    gpu.expect(
        &vga.snapshot(),
        [16, 4],
        &expected(&[23..39, 15..31, 15..31, 47..63]),
    )
    .await;
    indexed(&vga.io, 0x3d4, 8, 0);
    attribute(&vga.io, 0x10, 0x21);

    // Dividing the vertical counter doubles the display and split positions,
    // while the row-scan counter still advances once per physical scanline.
    indexed(&vga.io, 0x3d4, 0x17, 0xe7);
    gpu.expect(
        &vga.snapshot(),
        [16, 4],
        &expected(&[15..31, 0..16, 32..48, 64..80]),
    )
    .await;

    // Count-by-two repeats whole eight-pixel plane groups, not individual bits.
    attribute(&vga.io, 0x13, 0);
    indexed(&vga.io, 0x3d4, 0x18, 255);
    indexed(&vga.io, 0x3d4, 0x17, 0xeb);
    gpu.expect(
        &vga.snapshot(),
        [16, 2],
        &expected(&[8..16, 8..16, 40..48, 40..48]),
    )
    .await;

    // Other shifter/attribute combinations must not be mistaken for planar16.
    for (shift, mode) in [(0x20, 0x41), (0x40, 1), (0, 0x41)] {
        indexed(&vga.io, 0x3ce, 5, shift);
        attribute(&vga.io, 0x10, mode);
        gpu.expect(&vga.snapshot(), [16, 4], &[0, 0, 0, 255].repeat(64))
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
