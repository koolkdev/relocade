//! Literal blanking edges, independent of the CPU text reference.
use super::{common, graphics::Gpu};
use common::support::{
    indexed256, planar16,
    registers::{indexed, palette},
};
use relocade_vga::Vga;
use std::rc::Rc;

fn device(layout: u8, columns: u8, lines: u8) -> Vga<common::ManualClock> {
    let mut vga = Vga::with_clock(Rc::new(common::ManualClock::default()));
    match layout {
        0 | 1 => indexed(&mut vga, 0x3c4, 1, layout), // Nine/eight-dot text.
        2 => planar16::program(&mut vga),
        3 => indexed256::program(&mut vga),
        _ => unreachable!(),
    }
    for (index, value) in [
        (0x11, 0),
        (0, 45),
        (1, columns - 1),
        (2, 255),
        (6, lines - 2),
        (7, 0),
        (9, 0),
        (0x12, lines - 1),
        (0x15, 255),
        (0x18, 255),
    ] {
        common::crtc(&mut vga, index, value);
    }
    // Every zero-filled pixel uses this bright DAC entry. Blank output must
    // still be opaque black, not palette color zero or the overscan color.
    palette(&mut vga, 0, [63, 63, 63]);
    vga
}

fn expected(size: [u32; 2], blank: impl Fn(u32, u32) -> bool) -> Vec<u8> {
    let blank = &blank;
    (0..size[1])
        .flat_map(|y| {
            (0..size[0]).flat_map(move |x| {
                if blank(x, y) {
                    [0, 0, 0, 255]
                } else {
                    [255; 4]
                }
            })
        })
        .collect()
}

async fn check_horizontal() {
    let mut gpu = Gpu::new().await;
    for layout in 0..4 {
        let mut vga = device(layout, 80, 2);
        common::crtc(&mut vga, 0, 95); // 100 character clocks per line.
        let clock_width = if layout == 0 { 9 } else { 8 };
        let pixel_width = clock_width / if layout == 3 { 2 } else { 1 };
        let size = [80 * pixel_width, 2];
        for (start, end, skew, blank) in [
            (2, 5, 0, 3..6),
            (30, 34, 0, 31..35), // End bit five is in CRTC05, not CRTC03.
            (62, 1, 0, 63..66),  // The six-bit end comparator wraps within a line.
            (97, 1, 0, 0..2),    // Blanking crosses the total-counter reset.
            (2, 5, 2, 1..4),     // Display-enable skew moves the crop origin.
        ] {
            common::crtc(&mut vga, 2, start);
            common::crtc(&mut vga, 3, 0x80 | (skew << 5) | (end & 31));
            common::crtc(&mut vga, 5, (end & 32) << 2);
            let capture = common::fixture::capture(&vga);
            gpu.expect(
                &capture,
                size,
                &expected(size, |x, _| blank.contains(&(x / pixel_width))),
            )
            .await;
        }
        // Only registers change between these captures; retaining the earlier
        // one must retain its blanking even after later state has been uploaded.
        let earlier = common::fixture::capture(&vga);
        common::crtc(&mut vga, 2, 255); // Unreachable start.
        gpu.expect(
            &common::fixture::capture(&vga),
            size,
            &expected(size, |_, _| false),
        )
        .await;
        gpu.expect(
            &earlier,
            size,
            &expected(size, |x, _| (1..4).contains(&(x / pixel_width))),
        )
        .await;
    }
}

async fn check_vertical() {
    let mut gpu = Gpu::new().await;
    let mut vga = device(3, 1, 20);
    for (start, end, repeat, divisor, height, step, blank_rows) in [
        (2, 4, 1, 1, 20, 1, vec![3, 4]),
        (2, 0x84, 2, 1, 20, 1, vec![3, 4]), // End bit seven is ignored.
        (2, 4, 2, 2, 20, 2, vec![3, 4]),
        (1, 4, 2, 1, 20, 1, vec![2, 3, 4]), // Only the end cuts a repeated pair.
        (1, 3, 2, 1, 10, 2, vec![1]),
        (17, 1, 2, 1, 10, 2, vec![0, 9]), // Total reset before the end match.
        (17, 19, 2, 1, 10, 2, vec![9]),   // End at total has no wrapped head.
        (19, 1, 2, 1, 10, 2, vec![0]),    // Start on the last counter value.
        (20, 1, 2, 1, 10, 2, vec![]),     // Start is never reached.
        (2, 72, 2, 1, 10, 2, (0..10).collect()), // End is never reached.
        (2, 2, 2, 1, 10, 2, (0..10).collect()), // Simultaneous set wins.
    ] {
        for (index, value) in [
            (9, if repeat == 2 { 0x80 } else { 0 }),
            (0x17, if divisor == 2 { 0xa7 } else { 0xa3 }),
            (0x15, start),
            (0x16, end),
        ] {
            common::crtc(&mut vga, index, value);
        }
        let capture = common::fixture::capture(&vga);
        assert_eq!(capture.display.geometry().scanline_step, step);
        let size = [4, height];
        gpu.expect(
            &capture,
            size,
            &expected(size, |_, y| blank_rows.contains(&y)),
        )
        .await;
    }
    // Both vertical-start overflow bits participate: blank counts 771..774.
    for (index, value) in [
        (6, 0x1e),
        (7, 0x6b),
        (9, 0x20),
        (0x12, 0x07),
        (0x15, 2),
        (0x16, 5),
        (0x17, 0xa3),
    ] {
        common::crtc(&mut vga, index, value);
    }
    let size = [4, 776];
    gpu.expect(
        &common::fixture::capture(&vga),
        size,
        &expected(size, |_, y| (771..774).contains(&y)),
    )
    .await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn horizontal_blanking_is_shared_by_text_and_graphics() {
    pollster::block_on(check_horizontal());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn horizontal_blanking_is_shared_by_text_and_graphics() {
    check_horizontal().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn vertical_blanking_preserves_edges_and_counter_wrap() {
    pollster::block_on(check_vertical());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn vertical_blanking_preserves_edges_and_counter_wrap() {
    check_vertical().await;
}
