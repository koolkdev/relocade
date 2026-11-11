use super::{
    common,
    graphics::{load_palette, pixels, Gpu},
};
use crate::common::fixture::ConnectedVga;
use common::support::{
    indexed256::{program, unchain},
    registers::{attribute, indexed},
};
use relocade_clock_test_support::ManualClock;

fn start(io: &relocade_machine::IoBus, address: u16) {
    let [high, low] = address.to_be_bytes();
    indexed(io, 0x3d4, 12, high);
    indexed(io, 0x3d4, 13, low);
}

async fn check_pages() {
    let mut gpu = Gpu::new().await;
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(ConnectedVga::new(clock.clock()));
    program(&vga.io);
    unchain(&vga.io);
    load_palette(&vga.io);
    // A wider virtual row leaves room to scroll the 320-pixel display. Four
    // pages cover the entire plane, with the last page wrapping through zero.
    indexed(&vga.io, 0x3d4, 0x13, 44); // 88 plane bytes = 352 pixels per row.
    for plane in 0..4u8 {
        indexed(&vga.io, 0x3c4, 2, 1 << plane);
        for offset in 0..65536u32 {
            let color = (offset * 7 + (offset >> 8) * 29 + u32::from(plane) * 53) as u8;
            vga.io.write_mmio(0xa0000 + offset, 1, u64::from(color));
        }
    }
    let expected = |base: u16, pan: u32, height: u32| {
        pixels((0..height).flat_map(move |y| {
            (0..320).map(move |x| {
                let x = x + pan;
                let address = base.wrapping_add((y * 88 + x / 4) as u16);
                (u32::from(address) * 7 + u32::from(address >> 8) * 29 + (x & 3) * 53) as u8
            })
        }))
    };
    let source = vga.device.borrow().frames();
    let mut retained = Vec::new();
    for (frame, base) in [0, 0x4000, 0x8000, 0xc000].into_iter().enumerate() {
        start(&vga.io, base);
        // Programmed bytes already changed, but immediate snapshots still use
        // the previous latch. The shader must not decode the raw start bytes.
        let previous_base = frame.saturating_sub(1) as u16 * 0x4000;
        gpu.expect(
            &vga.snapshot(),
            [320, 200],
            &expected(previous_base, 0, 200),
        )
        .await;
        clock.advance_to(clock.at(common::boundary(frame as u64 + 1)));
        vga.io.read_port(0x3da, 1);
        common::service_video(video.as_mut());
        let capture = source.try_recv().unwrap();
        gpu.expect(&capture, [320, 200], &expected(base, 0, 200))
            .await;
        retained.push(capture);
    }
    // Program a 525-line raster with 480 visible, repeated scanlines.
    vga.io.write_port(0x3c2, 1, 0xe3);
    for (index, value) in [
        (6, 0x0b),
        (7, 0x3e),
        (0x10, 0xea),
        (0x12, 0xdf),
        (0x15, 0xe7),
        (0x16, 4),
        (0x11, 0x8c),
    ] {
        indexed(&vga.io, 0x3d4, index, value);
    }
    gpu.expect(&vga.snapshot(), [320, 240], &expected(0xc000, 0, 240))
        .await;
    let reset_at = common::boundary(4);
    program(&vga.io);
    unchain(&vga.io);
    indexed(&vga.io, 0x3d4, 0x13, 44);
    // A one-byte display-start shift scrolls four pixels. Pel panning then
    // crosses the plane group without changing which byte each plane stores.
    start(&vga.io, 0x8001);
    attribute(&vga.io, 0x13, 6);
    clock.advance_to(clock.at(reset_at + common::boundary(1)));
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    let panned = source.try_recv().unwrap();
    let mut changed = expected(0x8001, 3, 200);
    gpu.expect(&panned, [320, 200], &changed).await;

    // Change one plane at two nonadjacent physical pages and publish a diff.
    indexed(&vga.io, 0x3c4, 2, 4);
    for (x, y) in [(3, 1), (107, 101)] {
        let panned_x = x + 3;
        let offset = 0x8001 + y * 88 + panned_x / 4;
        vga.io.write_mmio(0xa0000 + offset, 1, 0xf1);
        changed[((y * 320 + x) * 4) as usize..((y * 320 + x) * 4 + 4) as usize]
            .copy_from_slice(&super::graphics::color(0xf1).1);
    }
    clock.advance_to(clock.at(reset_at + common::boundary(2)));
    vga.io.read_port(0x3da, 1);
    common::service_video(video.as_mut());
    gpu.expect(&source.try_recv().unwrap(), [320, 200], &changed)
        .await;
    drop(video);
    drop(vga);
    // Page flips changed only captured addresses; replay still restores the
    // old bytes after later sparse writes and device destruction.
    for (capture, base) in retained.iter().zip([0, 0x4000, 0x8000, 0xc000]) {
        gpu.expect(capture, [320, 200], &expected(base, 0, 200))
            .await;
    }
}

async fn check_split() {
    let mut gpu = Gpu::new().await;
    let clock = ManualClock::default();
    let vga = ConnectedVga::new(clock.clock());
    program(&vga.io);
    unchain(&vga.io);
    load_palette(&vga.io);
    for plane in 0..4u8 {
        indexed(&vga.io, 0x3c4, 2, 1 << plane);
        for address in 0..32u8 {
            vga.io.write_mmio(
                0xa0000 + u32::from(address),
                1,
                u64::from(address * 4 + plane),
            );
        }
    }
    start(&vga.io, 1);
    clock.advance_to(clock.at(common::boundary(1)));
    vga.io.read_port(0x3da, 1);
    // Eight pixels from a 24-pixel virtual row, beginning halfway through a
    // repeated row. Split after three physical scanlines; byte and pel pan both
    // cross four-plane groups above the split.
    for (index, value) in [
        (1, 1),
        (7, 0),
        (9, 1),
        (8, 0x21),
        (0x12, 5),
        (0x13, 3),
        (0x18, 2),
    ] {
        indexed(&vga.io, 0x3d4, index, value);
    }
    attribute(&vga.io, 0x13, 6);
    attribute(&vga.io, 0x10, 0x61);
    let reset_pan = vga.snapshot();
    attribute(&vga.io, 0x10, 0x41);
    let keep_pan = vga.snapshot();
    drop(vga);
    gpu.expect(
        &reset_pan,
        [8, 6],
        &pixels(
            [11..19, 35..43, 35..43, 0..8, 0..8, 24..32]
                .into_iter()
                .flatten(),
        ),
    )
    .await;
    gpu.expect(
        &keep_pan,
        [8, 6],
        &pixels(
            [11..19, 35..43, 35..43, 7..15, 7..15, 31..39]
                .into_iter()
                .flatten(),
        ),
    )
    .await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unchained_split_combines_preset_stride_and_both_panning_shifts() {
    pollster::block_on(check_split());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn unchained_split_combines_preset_stride_and_both_panning_shifts() {
    check_split().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unchained_plane_writes_pages_scroll_and_sparse_updates() {
    pollster::block_on(check_pages());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn unchained_plane_writes_pages_scroll_and_sparse_updates() {
    check_pages().await;
}
