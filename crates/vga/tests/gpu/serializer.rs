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

fn graphics(columns: u8) -> Vga<common::ManualClock> {
    let mut vga = Vga::with_clock(Rc::new(common::ManualClock::default()));
    program(&mut vga);
    load_palette(&mut vga);
    // One visible scanline, with blanking outside the small image.
    for (index, value) in [(1, columns - 1), (2, 255), (7, 0x10), (9, 0x40), (0x12, 0)] {
        indexed(&mut vga, 0x3d4, index, value);
    }
    vga
}

async fn check_load_and_count() {
    let mut gpu = Gpu::new().await;
    let mut vga = graphics(8);
    let first = [1, 2, 4, 8, 3, 6, 12, 9];
    let second = [8, 1, 2, 4, 9, 3, 6, 12];
    let third = [4, 8, 1, 2, 12, 9, 3, 6];
    let fourth = [2, 4, 8, 1, 6, 12, 9, 3];
    let memory = [
        first, [15; 8], [2; 8], [7; 8], [4; 8], [11; 8], [8; 8], [13; 8],
    ];
    write_pixels(&mut vga, memory.as_flattened());
    let ordinary = common::fixture::capture(&vga);
    gpu.expect(&ordinary, [64, 1], &pixels(memory.into_iter().flatten()))
        .await;

    // The CRTC counts every clock. Skipped loads must not fetch the next address.
    for (clocking, groups) in [
        (
            5,
            [
                first, second, [2; 8], [1; 8], [4; 8], [2; 8], [8; 8], [4; 8],
            ],
        ),
        (
            0x11,
            [first, second, third, fourth, [4; 8], [2; 8], [1; 8], [8; 8]],
        ),
        (
            0x15,
            [first, second, third, fourth, [4; 8], [2; 8], [1; 8], [8; 8]],
        ),
    ] {
        indexed(&mut vga, 0x3c4, 1, clocking);
        gpu.expect(
            &common::fixture::capture(&vga),
            [64, 1],
            &pixels(groups.into_iter().flatten()),
        )
        .await;
    }

    // Loading every four clocks with counting every two fetches addresses 0, 2.
    indexed(&mut vga, 0x3d4, 0x17, 0xeb);
    let groups = [first, second, third, fourth, [2; 8], [1; 8], [8; 8], [4; 8]];
    gpu.expect(
        &common::fixture::capture(&vga),
        [64, 1],
        &pixels(groups.into_iter().flatten()),
    )
    .await;
    // Counting every four instead fetches addresses 0, 1.
    indexed(&mut vga, 0x3d4, 0x17, 0xe3);
    indexed(&mut vga, 0x3d4, 0x14, 0x20);
    let groups = [
        first, second, third, fourth, [15; 8], [15; 8], [15; 8], [15; 8],
    ];
    gpu.expect(
        &common::fixture::capture(&vga),
        [64, 1],
        &pixels(groups.into_iter().flatten()),
    )
    .await;
    // Without delayed loads, count-by-four repeats the original eight dots.
    indexed(&mut vga, 0x3c4, 1, 1);
    let groups = [
        first, first, first, first, [15; 8], [15; 8], [15; 8], [15; 8],
    ];
    gpu.expect(
        &common::fixture::capture(&vga),
        [64, 1],
        &pixels(groups.into_iter().flatten()),
    )
    .await;
    drop(vga);
    gpu.expect(&ordinary, [64, 1], &pixels(memory.into_iter().flatten()))
        .await;
}

async fn check_packed_output() {
    let mut gpu = Gpu::new().await;
    let mut vga = graphics(4);
    // Each plane supplies different bit pairs/nibbles to the serializers.
    for (plane, byte) in [0x1b, 0xe4, 0x05, 0xaf].into_iter().enumerate() {
        indexed(&mut vga, 0x3c4, 2, 1 << plane);
        vga.write_mmio(0xa0000, 1, byte);
    }
    indexed(&mut vga, 0x3c4, 1, 0x11);
    indexed(&mut vga, 0x3ce, 5, 0x20);
    let interleaved = [
        0, 1, 6, 7, 11, 10, 13, 12, 0, 8, 3, 11, 13, 5, 14, 6, 0, 4, 9, 13, 14, 10, 7, 3, 0, 2, 12,
        14, 7, 5, 11, 9,
    ];
    gpu.expect(
        &common::fixture::capture(&vga),
        [32, 1],
        &pixels(interleaved),
    )
    .await;
    // Attribute plane masking applies to the serializer outputs, not VRAM planes.
    attribute(&mut vga, 0x12, 5);
    gpu.expect(
        &common::fixture::capture(&vga),
        [32, 1],
        &pixels(interleaved.map(|c| c & 5)),
    )
    .await;

    // In 256-color mode both nibbles pass through the four serializers. GC bit 6
    // still wins over bit 5, and neither changes the sequencer load interval.
    indexed(&mut vga, 0x3ce, 5, 0x60);
    attribute(&mut vga, 0x10, 0x41);
    let indexed = [
        0x1b, 0xe4, 0x05, 0xaf, 0x8d, 0x72, 0x0a, 0x5f, 0x4e, 0xb1, 0x05, 0xaf, 0x27, 0xd8, 0x0a,
        0x5f,
    ];
    gpu.expect(&common::fixture::capture(&vga), [16, 1], &pixels(indexed))
        .await;
    vga.write_port(0x3c6, 1, 0x7f);
    gpu.expect(
        &common::fixture::capture(&vga),
        [16, 1],
        &pixels(indexed.map(|c| c & 0x7f)),
    )
    .await;
}

async fn check_text() {
    let mut gpu = Gpu::new().await;
    let mut vga = common::fixture::text_device();
    common::fixture::set_cell(&mut vga, 0, b'F', 0x6f);
    common::fixture::set_cell(&mut vga, 1, 0xdb, 0x09);
    let expected = super::software::render(&vga.snapshot());
    for clocking in [5, 0x11, 0x15] {
        indexed(&mut vga, 0x3c4, 1, clocking);
        gpu.expect(&common::fixture::capture(&vga), [640, 200], &expected)
            .await;
    }
}

async fn check_byte_pan() {
    let mut gpu = Gpu::new().await;
    let mut vga = graphics(2);
    write_pixels(
        &mut vga,
        &[7, 6, 5, 4, 11, 10, 9, 8, 2, 2, 2, 2, 2, 2, 2, 2],
    );
    indexed(&mut vga, 0x3c4, 1, 0x11);
    indexed(&mut vga, 0x3d4, 0x14, 0x20);
    indexed(&mut vga, 0x3d4, 8, 0x60);
    attribute(&mut vga, 0x13, 7);
    // Byte pan 3 plus pel pan 7 starts at the last dot of the 32-dot load.
    gpu.expect(
        &common::fixture::capture(&vga),
        [16, 1],
        &pixels([1, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1]),
    )
    .await;
    // The same pan still precedes address counting when loads occur every clock.
    indexed(&mut vga, 0x3c4, 1, 1);
    gpu.expect(
        &common::fixture::capture(&vga),
        [16, 1],
        &pixels([8, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2]),
    )
    .await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn serializer_loading_is_independent_of_crtc_counting() {
    pollster::block_on(check_load_and_count());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn serializer_loading_is_independent_of_crtc_counting() {
    check_load_and_count().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn serializer_chaining_follows_graphics_bit_packing() {
    pollster::block_on(check_packed_output());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn serializer_chaining_follows_graphics_bit_packing() {
    check_packed_output().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn byte_pan_advances_within_a_serializer_load() {
    pollster::block_on(check_byte_pan());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn byte_pan_advances_within_a_serializer_load() {
    check_byte_pan().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn text_bypasses_graphics_serializers() {
    pollster::block_on(check_text());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn text_bypasses_graphics_serializers() {
    check_text().await;
}
