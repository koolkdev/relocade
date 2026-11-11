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

async fn check_blink() {
    let mut gpu = Gpu::new().await;
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(ConnectedVga::new(clock.clock()));
    program(&vga.io);
    load_palette(&vga.io);
    // Retain 800×525 timing, but display only the first sixteen pixels.
    for (index, value) in [(1, 1), (7, 0x3c), (0x12, 0)] {
        indexed(&vga.io, 0x3d4, index, value);
    }
    write_pixels(&vga.io, &(0..16).collect::<Vec<_>>());
    attribute(&vga.io, 0x10, 9);
    vga.io.read_port(0x3da, 1); // Finish register access before the first publication.
    common::service_video(video.as_mut());
    clock.advance_to(clock.at(common::dots(490 * 800, 25_175_000)));
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    let source = vga.device.borrow().frames();
    let on = source.try_recv().unwrap();
    assert!(on.vga().unwrap().attribute_blink_on);
    let color_on = pixels([8, 9, 10, 11, 12, 13, 14, 15, 0, 1, 2, 3, 4, 5, 6, 7]);
    gpu.expect(&on, [16, 1], &color_on).await;

    clock.advance_to(clock.at(common::dots((490 + 525) * 800, 25_175_000)));
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    assert!(source.is_empty());
    // The cursor phase changes at retrace eight, but graphics use attributes.
    clock.advance_to(clock.at(common::dots((490 + 7 * 525) * 800, 25_175_000)));
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    let cursor_off = source.try_recv().unwrap();
    assert!(!cursor_off.vga().unwrap().cursor_blink_on);
    assert!(cursor_off.vga().unwrap().attribute_blink_on);
    gpu.expect(&cursor_off, [16, 1], &color_on).await;
    clock.advance_to(clock.at(common::dots((490 + 15 * 525) * 800, 25_175_000)));
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    let off = source.try_recv().unwrap();
    assert!(!off.vga().unwrap().attribute_blink_on);
    for plane in 0..4 {
        assert!(std::ptr::eq(on.plane(plane), off.plane(plane)));
    }
    let upper = pixels([8, 9, 10, 11, 12, 13, 14, 15].repeat(2));
    gpu.expect(&off, [16, 1], &upper).await;

    attribute(&vga.io, 0x10, 0x0b); // Monochrome blinks independently of bit 3.
    gpu.expect(&vga.snapshot(), [16, 1], &upper).await;
    clock.advance_to(clock.at(common::dots((490 + 31 * 525) * 800, 25_175_000)));
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    let mono_on = source.try_recv().unwrap();
    assert!(mono_on.vga().unwrap().attribute_blink_on);
    gpu.expect(
        &mono_on,
        [16, 1],
        &pixels([0, 1, 2, 3, 4, 5, 6, 7].repeat(2)),
    )
    .await;

    // Plane masking precedes blink. Removing bit 3 disables color-graphics
    // blinking, while monochrome still toggles the palette half.
    attribute(&vga.io, 0x12, 7);
    attribute(&vga.io, 0x10, 9);
    gpu.expect(&vga.snapshot(), [16, 1], &upper).await;
    attribute(&vga.io, 0x12, 5);
    attribute(&vga.io, 0x10, 0x0b);
    gpu.expect(
        &vga.snapshot(),
        [16, 1],
        &pixels([0, 1, 0, 1, 4, 5, 4, 5].repeat(2)),
    )
    .await;

    attribute(&vga.io, 0x10, 1); // Disabling blink restores the ordinary index.
    attribute(&vga.io, 0x12, 15);
    gpu.expect(&vga.snapshot(), [16, 1], &pixels(0..16)).await;
    drop(video);
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
