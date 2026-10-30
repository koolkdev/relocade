use super::{
    common,
    graphics::{load_palette, pixels, Gpu},
};
use common::support::indexed256::{attribute, indexed, program, unchain};
use relocade_machine::Device;
use relocade_vga::Vga;
use std::rc::Rc;

fn start(vga: &mut impl Device, address: u16) {
    let [high, low] = address.to_be_bytes();
    indexed(vga, 0x3d4, 12, high);
    indexed(vga, 0x3d4, 13, low);
}

async fn check_pages() {
    let mut gpu = Gpu::new().await;
    let clock = Rc::new(common::ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    program(&mut vga);
    unchain(&mut vga);
    load_palette(&mut vga);
    // A wider virtual row leaves room to scroll the 320-pixel display. Four
    // pages cover the entire plane, with the last page wrapping through zero.
    indexed(&mut vga, 0x3d4, 0x13, 44); // 88 plane bytes = 352 pixels per row.
    for plane in 0..4u8 {
        indexed(&mut vga, 0x3c4, 2, 1 << plane);
        for offset in 0..65536u32 {
            let color = (offset * 7 + (offset >> 8) * 29 + u32::from(plane) * 53) as u8;
            vga.write_mmio(0xa0000 + offset, 1, u64::from(color));
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
    let source = vga.frame_source();
    let mut retained = Vec::new();
    for (frame, base) in [0, 0x4000, 0x8000, 0xc000].into_iter().enumerate() {
        start(&mut vga, base);
        clock.set(common::boundary(frame as u64 + 1));
        vga.read_port(0x3da, 1);
        let capture = source.current_capture().unwrap();
        gpu.expect(&capture, [320, 200], &expected(base, 0, 200))
            .await;
        retained.push(capture);
    }
    // Program a 525-line raster with 480 visible, repeated scanlines. Geometry
    // follows these registers; publication still uses the documented fixed clock.
    vga.write_port(0x3c2, 1, 0xe3);
    for (index, value) in [
        (6, 0x0b),
        (7, 0x3e),
        (0x10, 0xea),
        (0x12, 0xdf),
        (0x15, 0xe7),
        (0x16, 4),
        (0x11, 0x8c),
    ] {
        indexed(&mut vga, 0x3d4, index, value);
    }
    gpu.expect(
        &common::fixture::capture(&vga),
        [320, 240],
        &expected(0xc000, 0, 240),
    )
    .await;
    program(&mut vga);
    unchain(&mut vga);
    indexed(&mut vga, 0x3d4, 0x13, 44);
    // A one-byte display-start shift scrolls four pixels. Pel panning then
    // crosses the plane group without changing which byte each plane stores.
    start(&mut vga, 0x8001);
    attribute(&mut vga, 0x13, 6);
    clock.set(common::boundary(5));
    vga.read_port(0x3da, 1);
    let panned = source.current_capture().unwrap();
    let mut changed = expected(0x8001, 3, 200);
    gpu.expect(&panned, [320, 200], &changed).await;

    // Change one plane at two nonadjacent physical pages and publish a diff.
    indexed(&mut vga, 0x3c4, 2, 4);
    for (x, y) in [(3, 1), (107, 101)] {
        let panned_x = x + 3;
        let offset = 0x8001 + y * 88 + panned_x / 4;
        vga.write_mmio(0xa0000 + offset, 1, 0xf1);
        changed[((y * 320 + x) * 4) as usize..((y * 320 + x) * 4 + 4) as usize]
            .copy_from_slice(&super::graphics::color(0xf1).1);
    }
    clock.set(common::boundary(6));
    vga.read_port(0x3da, 1);
    gpu.expect(&source.current_capture().unwrap(), [320, 200], &changed)
        .await;
    drop(vga);
    // Page flips changed only captured addresses; replay still restores the
    // old bytes after later sparse writes and device destruction.
    for (capture, base) in retained.iter().zip([0, 0x4000, 0x8000, 0xc000]) {
        gpu.expect(capture, [320, 200], &expected(base, 0, 200))
            .await;
    }
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
