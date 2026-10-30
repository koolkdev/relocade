use super::{common, run, Gpu};

#[test]
fn bios_page_selection_and_scrolling_reach_captured_planar_pixels() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let before = run(
            &mut dos,
            &[
                0xb8, 0x0d, 0, 0xcd, 0x10, // Mode 0Dh.
                0xb8, 4, 0x0c, 0xbb, 0, 1, // Red pixel on page one.
                0xb9, 0, 0, 0xba, 8, 0, 0xcd, 0x10, 0xb8, 1, 5, 0xcd,
                0x10, // Display page one.
                0xc3,
            ],
        )
        .await;
        let after = run(
            &mut dos,
            &[
                0xb8, 1, 6, 0xbb, 0, 2, // Scroll up one character row, green fill.
                0xb9, 0, 0, 0xba, 0, 1, 0xcd, 0x10, // Window: rows 0..1, column 0.
                0xc3,
            ],
        )
        .await;
        drop(dos);
        let mut gpu = Gpu::new().await;
        for (capture, scrolled) in [(&after, true), (&before, false)] {
            let image = gpu.render(capture, [320, 200]).await;
            for y in 0..200 {
                for x in 0..320 {
                    let rgb = if x == 0 && y == if scrolled { 0 } else { 8 } {
                        [170, 0, 0, 255]
                    } else if scrolled && x < 8 && (8..16).contains(&y) {
                        [0, 170, 0, 255]
                    } else {
                        [0, 0, 0, 255]
                    };
                    assert_eq!(&image[(y * 320 + x) * 4..][..4], &rgb, "({x}, {y})");
                }
            }
        }
    });
}
