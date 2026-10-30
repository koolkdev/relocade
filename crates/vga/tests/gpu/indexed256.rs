use super::{common, read_pixels};
use common::support::indexed256::{attribute, indexed, palette, program};
use relocade_machine::Device;
use relocade_vga::{CapturedFrame, Vga, VgaRenderer};
use std::rc::Rc;

fn color(index: u8) -> ([u8; 3], [u8; 4]) {
    // Eight exact six-bit levels and their independently listed RGBA values.
    let dac = [0, 9, 18, 27, 36, 45, 54, 63];
    let rgba = [0, 36, 73, 109, 146, 182, 219, 255];
    let r = usize::from(index & 7);
    let g = usize::from((index >> 3) & 7);
    let b = usize::from(index >> 6);
    (
        [dac[r], dac[g], [0, 21, 42, 63][b]],
        [rgba[r], rgba[g], [0, 85, 170, 255][b], 255],
    )
}

fn load_palette(vga: &mut impl Device) {
    for index in 0..=255 {
        palette(vga, index, color(index).0);
    }
}

fn pixels(indices: impl IntoIterator<Item = u8>) -> Vec<u8> {
    indices
        .into_iter()
        .flat_map(|index| color(index).1)
        .collect()
}

async fn expect_image(
    renderer: &mut VgaRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    capture: &CapturedFrame,
    size: [u32; 2],
    expected: &[u8],
) {
    renderer.upload(queue, capture);
    let mut encoder = device.create_command_encoder(&Default::default());
    let frame = renderer.encode(&mut encoder).unwrap();
    queue.submit([encoder.finish()]);
    assert_eq!(frame.image.size(), size);
    let actual = read_pixels(device, queue, frame.image.texture()).await;
    assert_eq!(actual.len(), expected.len());
    for (index, (actual, expected)) in actual
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected.as_chunks::<4>().0)
        .enumerate()
    {
        assert_eq!(
            actual,
            expected,
            "pixel ({}, {})",
            index % size[0] as usize,
            index / size[0] as usize
        );
    }
}

async fn check_full_frame() {
    let adapter = wgpu::Instance::default()
        .request_adapter(&Default::default())
        .await
        .unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    let clock = Rc::new(common::ManualClock::default());
    let mut vga = common::text_vga(clock.clone());
    common::fixture::set_cell(&mut vga, 0, 0xdb, 4);
    common::crtc(&mut vga, 10, 0x20);
    let text = common::fixture::capture(&vga);
    let text_pixels = super::software::render(&text.display);
    expect_image(
        &mut renderer,
        &device,
        &queue,
        &text,
        [640, 200],
        &text_pixels,
    )
    .await;

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
    expect_image(
        &mut renderer,
        &device,
        &queue,
        &first,
        [320, 200],
        &original,
    )
    .await;

    // Sparse writes cross planes, pages and both high chain-4 address bits.
    for offset in [0, 1, 16383, 16384, 32767, 32768, 49152, 63999] {
        indices[offset] ^= 0xff;
        vga.write_mmio(0xa0000 + offset as u32, 1, u64::from(indices[offset]));
    }
    clock.set(common::boundary(2));
    vga.read_port(0x3da, 1);
    let changed = source.current_capture().unwrap();
    expect_image(
        &mut renderer,
        &device,
        &queue,
        &changed,
        [320, 200],
        &pixels(indices.iter().copied()),
    )
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
    expect_image(
        &mut renderer,
        &device,
        &queue,
        &newest,
        [320, 200],
        &expected,
    )
    .await;

    // CPU routing does not select the display layout; Color Select is unused
    // in eight-bit graphics. The DAC mask still applies to the complete index.
    indexed(&mut vga, 0x3c4, 4, 6);
    attribute(&mut vga, 0x14, 15);
    vga.write_port(0x3c6, 1, 0x7f);
    let masked = common::fixture::capture(&vga);
    let expected = pixels(indices.iter().map(|index| index & 0x7f));
    expect_image(
        &mut renderer,
        &device,
        &queue,
        &masked,
        [320, 200],
        &expected,
    )
    .await;
    drop(vga);
    expect_image(
        &mut renderer,
        &device,
        &queue,
        &first,
        [320, 200],
        &original,
    )
    .await;
    expect_image(
        &mut renderer,
        &device,
        &queue,
        &text,
        [640, 200],
        &text_pixels,
    )
    .await;
}

async fn check_addressing() {
    let adapter = wgpu::Instance::default()
        .request_adapter(&Default::default())
        .await
        .unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    let mut vga = Vga::with_clock(Rc::new(common::ManualClock::default()));
    program(&mut vga);
    load_palette(&mut vga);
    for offset in 0..256 {
        vga.write_mmio(0xa0000 + offset, 1, u64::from(offset));
    }
    vga.write_mmio(0xafffc, 4, 0xfffefdfc);
    for (index, value) in [(1, 1), (7, 0), (9, 1), (13, 1), (0x12, 3), (0x13, 3)] {
        common::crtc(&mut vga, index, value);
    }
    let first = common::fixture::capture(&vga);
    expect_image(
        &mut renderer,
        &device,
        &queue,
        &first,
        [8, 2],
        &pixels((4..12).chain(28..36)),
    )
    .await;
    attribute(&mut vga, 0x13, 6); // Three pixels, crossing a four-plane group.
    let panned = common::fixture::capture(&vga);
    expect_image(
        &mut renderer,
        &device,
        &queue,
        &panned,
        [8, 2],
        &pixels((7..15).chain(31..39)),
    )
    .await;

    attribute(&mut vga, 0x10, 0x61);
    common::crtc(&mut vga, 0x18, 0); // Split in the middle of the repeated pair.
    let split = common::fixture::capture(&vga);
    expect_image(
        &mut renderer,
        &device,
        &queue,
        &split,
        [8, 4],
        &pixels((7..15).chain(0..8).chain(0..8).chain(24..32)),
    )
    .await;
    attribute(&mut vga, 0x13, 0);
    common::crtc(&mut vga, 0x18, 0xff);
    common::crtc(&mut vga, 8, 1);
    let preset = common::fixture::capture(&vga);
    expect_image(
        &mut renderer,
        &device,
        &queue,
        &preset,
        [8, 4],
        &pixels((4..12).chain(28..36).chain(28..36).chain(52..60)),
    )
    .await;

    for (index, value) in [(8, 0), (9, 3), (0x12, 5)] {
        common::crtc(&mut vga, index, value);
    }
    let partial = common::fixture::capture(&vga);
    expect_image(
        &mut renderer,
        &device,
        &queue,
        &partial,
        [8, 6],
        &pixels((4..12).cycle().take(32).chain((28..36).cycle().take(16))),
    )
    .await;

    for (index, value) in [(9, 1), (0x12, 1), (12, 0x3f), (13, 0xff)] {
        common::crtc(&mut vga, index, value);
    }
    let wrapped = common::fixture::capture(&vga);
    expect_image(
        &mut renderer,
        &device,
        &queue,
        &wrapped,
        [8, 1],
        &pixels([252, 253, 254, 255, 0, 1, 2, 3]),
    )
    .await;
    indexed(&mut vga, 0x3c4, 1, 0x21);
    let blanked = common::fixture::capture(&vga);
    expect_image(
        &mut renderer,
        &device,
        &queue,
        &blanked,
        [8, 1],
        &[0, 0, 0, 255].repeat(8),
    )
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
