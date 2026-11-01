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
use std::{rc::Rc, sync::Arc};

async fn check_blink() {
    let mut gpu = Gpu::new().await;
    let clock = Rc::new(common::ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    program(&mut vga);
    load_palette(&mut vga);
    // Retain 800×525 timing, but display only the first sixteen pixels.
    for (index, value) in [(1, 1), (7, 0x3c), (0x12, 0)] {
        indexed(&mut vga, 0x3d4, index, value);
    }
    write_pixels(&mut vga, &(0..16).collect::<Vec<_>>());
    attribute(&mut vga, 0x10, 9);
    vga.read_port(0x3da, 1); // Finish register access before the first publication.
    clock.set(common::dots(490 * 800, 25_175_000));
    vga.read_port(0x3da, 1);
    let source = vga.frame_source();
    let on = source.current_capture().unwrap();
    assert!(on.display.blink_on);
    let color_on = pixels([8, 9, 10, 11, 12, 13, 14, 15, 0, 1, 2, 3, 4, 5, 6, 7]);
    gpu.expect(&on, [16, 1], &color_on).await;

    clock.set(common::dots((490 + 525) * 800, 25_175_000));
    vga.read_port(0x3da, 1);
    assert!(Arc::ptr_eq(&on, &source.current_capture().unwrap()));
    clock.set(common::dots((490 + 15 * 525) * 800, 25_175_000));
    vga.read_port(0x3da, 1);
    let off = source.current_capture().unwrap();
    assert!(!off.display.blink_on);
    for plane in 0..4 {
        assert!(std::ptr::eq(
            on.display.plane(plane),
            off.display.plane(plane)
        ));
    }
    let upper = pixels([8, 9, 10, 11, 12, 13, 14, 15].repeat(2));
    gpu.expect(&off, [16, 1], &upper).await;

    attribute(&mut vga, 0x10, 0x0b); // Monochrome blinks independently of bit 3.
    gpu.expect(&common::fixture::capture(&vga), [16, 1], &upper)
        .await;
    clock.set(common::dots((490 + 31 * 525) * 800, 25_175_000));
    vga.read_port(0x3da, 1);
    let mono_on = source.current_capture().unwrap();
    assert!(mono_on.display.blink_on);
    gpu.expect(
        &mono_on,
        [16, 1],
        &pixels([0, 1, 2, 3, 4, 5, 6, 7].repeat(2)),
    )
    .await;

    // Plane masking precedes blink. Removing bit 3 disables color-graphics
    // blinking, while monochrome still toggles the palette half.
    attribute(&mut vga, 0x12, 7);
    attribute(&mut vga, 0x10, 9);
    gpu.expect(&common::fixture::capture(&vga), [16, 1], &upper)
        .await;
    attribute(&mut vga, 0x12, 5);
    attribute(&mut vga, 0x10, 0x0b);
    gpu.expect(
        &common::fixture::capture(&vga),
        [16, 1],
        &pixels([0, 1, 0, 1, 4, 5, 4, 5].repeat(2)),
    )
    .await;

    attribute(&mut vga, 0x10, 1); // Disabling blink restores the ordinary index.
    attribute(&mut vga, 0x12, 15);
    gpu.expect(&common::fixture::capture(&vga), [16, 1], &pixels(0..16))
        .await;
    drop(vga);
    gpu.expect(&off, [16, 1], &upper).await;
    gpu.expect(&on, [16, 1], &color_on).await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn graphics_blink_palette_masking_and_retained_phases() {
    pollster::block_on(check_blink());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn graphics_blink_palette_masking_and_retained_phases() {
    check_blink().await;
}
