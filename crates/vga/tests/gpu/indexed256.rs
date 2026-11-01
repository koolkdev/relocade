use super::{
    common,
    graphics::{color, load_palette, pixels, Gpu},
};
use common::support::{
    indexed256::program,
    registers::{attribute, indexed, palette},
};
use relocade_machine::Device;
use relocade_vga::Vga;
use std::rc::Rc;

async fn check_full_frame() {
    let mut gpu = Gpu::new().await;
    let clock = Rc::new(common::ManualClock::default());
    let mut vga = common::text_vga(clock.clone());
    common::fixture::set_cell(&mut vga, 0, 0xdb, 4);
    common::crtc(&mut vga, 10, 0x20);
    let text = common::fixture::capture(&vga);
    let text_pixels = super::software::render(&text.display);
    gpu.expect(&text, [640, 200], &text_pixels).await;

    program(&mut vga);
    load_palette(&mut vga);
    let mut indices = Vec::with_capacity(64000);
    for y in 0..200 {
        for x in 0..320 {
            let value = (x * 3 + y * 17) as u8;
            vga.write_mmio(0xa0000 + y * 320 + x, 1, u64::from(value));
            indices.push(value);
        }
    }
    let source = vga.frame_source();
    clock.set(common::boundary(1));
    vga.read_port(0x3da, 1);
    let first = source.current_capture().unwrap();
    let original = pixels(indices.iter().copied());
    assert_eq!(first.display.geometry().pixel_aspect_ratio(), (5, 6));
    gpu.expect(&first, [320, 200], &original).await;

    // Sparse writes cross planes, pages and both high chain-4 address bits.
    for offset in [0, 1, 16383, 16384, 32767, 32768, 49152, 63999] {
        indices[offset] ^= 0xff;
        vga.write_mmio(0xa0000 + offset as u32, 1, u64::from(indices[offset]));
    }
    clock.set(common::boundary(2));
    vga.read_port(0x3da, 1);
    let changed = source.current_capture().unwrap();
    gpu.expect(&changed, [320, 200], &pixels(indices.iter().copied()))
        .await;

    vga.write_mmio(0xa0000 + 40000, 1, 0xe7);
    indices[40000] = 0xe7;
    clock.set(common::boundary(3));
    vga.read_port(0x3da, 1); // Skip this capture's pixel change.
    palette(&mut vga, 0xe7, [63, 63, 63]);
    clock.set(common::boundary(4));
    vga.read_port(0x3da, 1);
    let newest = source.current_capture().unwrap();
    let expected: Vec<u8> = indices
        .iter()
        .flat_map(|&index| {
            if index == 0xe7 {
                [255; 4]
            } else {
                color(index).1
            }
        })
        .collect();
    gpu.expect(&newest, [320, 200], &expected).await;

    // CPU routing does not select the display layout; Color Select is unused
    // in eight-bit graphics. The DAC mask still applies to the complete index.
    indexed(&mut vga, 0x3c4, 4, 6);
    attribute(&mut vga, 0x14, 15);
    vga.write_port(0x3c6, 1, 0x7f);
    let masked = common::fixture::capture(&vga);
    let expected = pixels(indices.iter().map(|index| index & 0x7f));
    gpu.expect(&masked, [320, 200], &expected).await;
    // Eight-bit pixels bypass the attribute palette, including graphics blink.
    for mode in [0x49, 0x4b] {
        attribute(&mut vga, 0x10, mode);
        gpu.expect(&common::fixture::capture(&vga), [320, 200], &expected)
            .await;
    }
    drop(vga);
    gpu.expect(&first, [320, 200], &original).await;
    gpu.expect(&text, [640, 200], &text_pixels).await;
}

async fn check_addressing() {
    let mut gpu = Gpu::new().await;
    let clock = Rc::new(common::ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    program(&mut vga);
    common::crtc(&mut vga, 13, 1);
    clock.set(common::boundary(1));
    vga.read_port(0x3da, 1); // Latch the start before changing the viewport.
    load_palette(&mut vga);
    for offset in 0..256 {
        vga.write_mmio(0xa0000 + offset, 1, u64::from(offset));
    }
    vga.write_mmio(0xafffc, 4, 0xfffefdfc);
    for (index, value) in [(1, 1), (7, 0), (9, 1), (13, 1), (0x12, 3), (0x13, 3)] {
        common::crtc(&mut vga, index, value);
    }
    let first = common::fixture::capture(&vga);
    gpu.expect(&first, [8, 2], &pixels((4..12).chain(28..36)))
        .await;
    attribute(&mut vga, 0x13, 6); // Three pixels, crossing a four-plane group.
    let panned = common::fixture::capture(&vga);
    gpu.expect(&panned, [8, 2], &pixels((7..15).chain(31..39)))
        .await;

    attribute(&mut vga, 0x10, 0x61);
    common::crtc(&mut vga, 0x18, 0); // Split in the middle of the repeated pair.
    let split = common::fixture::capture(&vga);
    gpu.expect(
        &split,
        [8, 4],
        &pixels((7..15).chain(0..8).chain(0..8).chain(24..32)),
    )
    .await;
    attribute(&mut vga, 0x13, 0);
    common::crtc(&mut vga, 0x18, 0xff);
    common::crtc(&mut vga, 8, 1);
    let preset = common::fixture::capture(&vga);
    gpu.expect(
        &preset,
        [8, 4],
        &pixels((4..12).chain(28..36).chain(28..36).chain(52..60)),
    )
    .await;

    for (index, value) in [(8, 0), (9, 3), (0x12, 5)] {
        common::crtc(&mut vga, index, value);
    }
    let partial = common::fixture::capture(&vga);
    gpu.expect(
        &partial,
        [8, 6],
        &pixels((4..12).cycle().take(32).chain((28..36).cycle().take(16))),
    )
    .await;

    for (index, value) in [(9, 1), (0x12, 1), (12, 0x3f), (13, 0xff)] {
        common::crtc(&mut vga, index, value);
    }
    clock.set(std::time::Duration::from_secs(1));
    vga.read_port(0x3da, 1);
    let wrapped = common::fixture::capture(&vga);
    gpu.expect(&wrapped, [8, 1], &pixels([252, 253, 254, 255, 0, 1, 2, 3]))
        .await;
    indexed(&mut vga, 0x3c4, 1, 0x21);
    let blanked = common::fixture::capture(&vga);
    gpu.expect(&blanked, [8, 1], &[0, 0, 0, 255].repeat(8))
        .await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn linear_pixels_palette_updates_and_retained_modes() {
    pollster::block_on(check_full_frame());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn linear_pixels_palette_updates_and_retained_modes() {
    check_full_frame().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn graphics_addresses_follow_stride_pan_split_and_wrap() {
    pollster::block_on(check_addressing());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn graphics_addresses_follow_stride_pan_split_and_wrap() {
    check_addressing().await;
}
