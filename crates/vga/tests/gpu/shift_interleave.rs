use super::{
    common,
    graphics::{load_palette, pixels, Gpu},
};
use common::support::{
    planar16::program,
    registers::{attribute, indexed},
};
use relocade_machine::Device;
use relocade_vga::Vga;
use std::rc::Rc;

async fn check_colors() {
    let mut gpu = Gpu::new().await;
    let mut vga = Vga::with_clock(Rc::new(common::ManualClock::default()));
    program(&mut vga);
    load_palette(&mut vga);
    for (index, value) in [(1, 1), (7, 0x10), (9, 0x40), (0x12, 0)] {
        indexed(&mut vga, 0x3d4, index, value);
    }
    indexed(&mut vga, 0x3ce, 5, 0x20);
    for (plane, bytes) in [[0x1b, 0x55], [0xe4, 0xaa], [0x05, 0x0f], [0xaf, 0xf0]]
        .into_iter()
        .enumerate()
    {
        indexed(&mut vga, 0x3c4, 2, 1 << plane);
        for (offset, byte) in bytes.into_iter().enumerate() {
            vga.write_mmio(0xa0000 + offset as u32, 1, byte);
        }
    }
    let original = common::fixture::capture(&vga);
    let colors = [0, 1, 6, 7, 11, 10, 13, 12, 1, 1, 13, 13, 14, 14, 2, 2];
    gpu.expect(&original, [16, 1], &pixels(colors)).await;
    // Mask assembled color bits, not the source memory planes.
    attribute(&mut vga, 0x12, 5);
    gpu.expect(
        &common::fixture::capture(&vga),
        [16, 1],
        &pixels(colors.map(|c| c & 5)),
    )
    .await;
    attribute(&mut vga, 0x12, 15);
    // Interleaving retains eight pixels per blanking character clock.
    for (index, value) in [(2, 0), (3, 0x81), (5, 0)] {
        indexed(&mut vga, 0x3d4, index, value);
    }
    let mut blanked = pixels(colors);
    blanked[8 * 4..].copy_from_slice(&[0, 0, 0, 255].repeat(8));
    gpu.expect(&common::fixture::capture(&vga), [16, 1], &blanked)
        .await;
    // Disable blanking for the remaining pixel cases.
    indexed(&mut vga, 0x3d4, 2, 255);
    // A pan crosses both the map0/2-to-map1/3 boundary and the next fetch.
    indexed(&mut vga, 0x3d4, 1, 0);
    attribute(&mut vga, 0x13, 3);
    gpu.expect(
        &common::fixture::capture(&vga),
        [8, 1],
        &pixels([7, 11, 10, 13, 12, 1, 1, 13]),
    )
    .await;
    attribute(&mut vga, 0x13, 0);
    indexed(&mut vga, 0x3d4, 1, 1);
    indexed(&mut vga, 0x3ce, 5, 0x60);
    attribute(&mut vga, 0x10, 0x41);
    gpu.expect(
        &common::fixture::capture(&vga),
        [8, 1],
        &pixels([0x1b, 0xe4, 0x05, 0xaf, 0x55, 0xaa, 0x0f, 0xf0]),
    )
    .await;
    drop(vga);
    gpu.expect(&original, [16, 1], &pixels(colors)).await;
}

async fn check_banked_rows() {
    let mut gpu = Gpu::new().await;
    let mut vga = Vga::with_clock(Rc::new(common::ManualClock::default()));
    program(&mut vga);
    load_palette(&mut vga);
    // CGA-compatible CPU odd/even access, word display addresses, RS0 on MA13.
    for (port, index, value) in [
        (0x3c4, 2, 3),
        (0x3c4, 4, 2),
        (0x3ce, 5, 0x30),
        (0x3ce, 6, 0x0f),
    ] {
        indexed(&mut vga, port, index, value);
    }
    for (index, value) in [
        (1, 1),
        (7, 0x10),
        (9, 0xc1),
        (0x12, 7),
        (0x13, 1),
        (0x17, 0xa2),
    ] {
        indexed(&mut vga, 0x3d4, index, value);
    }
    attribute(&mut vga, 0x12, 3);
    for (bank, bytes) in [
        [0x1b, 0xe4, 0x55, 0xaa, 0xff, 0, 0x6c, 0x93],
        [0xaa, 0x55, 0xe4, 0x1b, 0x93, 0x6c, 0, 0xff],
    ]
    .into_iter()
    .enumerate()
    {
        for (offset, byte) in bytes.into_iter().enumerate() {
            vga.write_mmio(0xb8000 + bank as u32 * 0x2000 + offset as u32, 1, byte);
        }
    }
    let rows = [
        [0, 1, 2, 3, 3, 2, 1, 0, 1, 1, 1, 1, 2, 2, 2, 2],
        [2, 2, 2, 2, 1, 1, 1, 1, 3, 2, 1, 0, 0, 1, 2, 3],
        [3, 3, 3, 3, 0, 0, 0, 0, 1, 2, 3, 0, 2, 1, 0, 3],
        [2, 1, 0, 3, 1, 2, 3, 0, 0, 0, 0, 0, 3, 3, 3, 3],
    ];
    let capture = common::fixture::capture(&vga);
    assert_eq!(capture.display.geometry().scanline_step, 2);
    gpu.expect(&capture, [16, 4], &pixels(rows.into_iter().flatten()))
        .await;
    // Count-by-two repeats a whole eight-pixel fetch, while RS0 still banks rows.
    indexed(&mut vga, 0x3d4, 0x17, 0xaa);
    let repeated = rows
        .iter()
        .flat_map(|row| row[..8].iter().copied().cycle().take(16));
    gpu.expect(&common::fixture::capture(&vga), [16, 4], &pixels(repeated))
        .await;
    indexed(&mut vga, 0x3d4, 0x17, 0xa2);
    // A split inside a doubled pair keeps every physical scanline and restarts RS0.
    indexed(&mut vga, 0x3d4, 7, 0);
    indexed(&mut vga, 0x3d4, 9, 0x81);
    indexed(&mut vga, 0x3d4, 0x18, 0);
    let split = [0, 0, 0, 1, 1, 2, 2, 3]
        .into_iter()
        .flat_map(|row| rows[row]);
    gpu.expect(&common::fixture::capture(&vga), [16, 8], &pixels(split))
        .await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shift_interleave_colors_pan_and_256_color_precedence() {
    pollster::block_on(check_colors());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn shift_interleave_colors_pan_and_256_color_precedence() {
    check_colors().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shift_interleave_banked_rows_counting_and_split() {
    pollster::block_on(check_banked_rows());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn shift_interleave_banked_rows_counting_and_split() {
    check_banked_rows().await;
}
