//! Guest BIOS calls and direct programming reach the complete nonblanked image.
use super::{common, planar16::COLORS, run, Gpu, TEXT_WITH_RED_BLOCK};
use std::ops::Range;

const BLACK: [u8; 4] = [0, 0, 0, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

// INT 10h/1001h selects the full overscan index; 1010h programs its DAC color.
const RED_BORDER: &[u8] = &[
    0xb8, 1, 0x10, 0xbb, 0, 0xb2, 0xcd, 0x10, // AC11 = B2h.
    0xb8, 0x10, 0x10, 0xbb, 0xb2, 0, 0x31, 0xc9, 0xba, 0, 63, 0xcd, 0x10, // DAC[B2h] = red.
];

fn expected_image(
    size: [u32; 2],
    active: [Range<u32>; 2],
    border: [u8; 4],
    pixel: impl Fn(u32, u32) -> [u8; 4],
) -> Vec<u8> {
    let mut expected = Vec::with_capacity((size[0] * size[1] * 4) as usize);
    for y in 0..size[1] {
        for x in 0..size[0] {
            let color = if active[0].contains(&x) && active[1].contains(&y) {
                pixel(x - active[0].start, y - active[1].start)
            } else {
                border
            };
            expected.extend_from_slice(&color);
        }
    }
    expected
}

fn assert_image(actual: &[u8], expected: &[u8], width: u32) {
    assert_eq!(actual.len(), expected.len());
    for (i, (actual, expected)) in actual
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
            i as u32 % width,
            i as u32 / width
        );
    }
}

#[test]
fn bios_borders_and_direct_dac_changes_survive_mode_transitions() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let mut gpu = Gpu::new().await;
        let mut retained = Vec::new();
        for (mode, size, active, mark) in [
            (3, [738, 414], [9..729, 7..407], [9, 16]),
            (0x12, [656, 496], [8..648, 8..488], [8, 1]),
            (0x13, [328, 414], [4..324, 7..407], [1, 2]),
        ] {
            let mut program = vec![0xb8, mode, 0, 0xcd, 0x10];
            if mode == 3 {
                program.extend_from_slice(&[
                    0xb8, 0, 1, 0xb9, 7, 0x20, 0xcd, 0x10, // Hide cursor.
                    0xb8, 0xdb, 9, 0xbb, 15, 0, 0xb9, 1, 0, 0xcd, 0x10, // White block.
                ]);
            } else {
                let byte = if mode == 0x13 { 15 } else { 255 };
                program.extend_from_slice(&[
                    0xb8, 0, 0xa0, 0x8e, 0xc0, // ES = A000.
                    0x26, 0xc6, 0x06, 0, 0, byte, // First byte/pixel is white.
                ]);
            }
            program.extend_from_slice(RED_BORDER);
            program.push(0xc3);
            let red = run(&mut dos, &program).await;
            let marked_pixel = |x, y| {
                if x < mark[0] && y < mark[1] {
                    [255; 4]
                } else {
                    BLACK
                }
            };
            let expected = expected_image(size, active.clone(), RED, marked_pixel);
            assert_image(&gpu.render(&red, size).await, &expected, size[0]);

            // Direct DAC writes update the same color selected by the BIOS.
            let green = run(
                &mut dos,
                &[
                    0xba, 0xc8, 3, 0xb0, 0xb2, 0xee, 0x42, // Select DAC B2h, then port 3C9h.
                    0xb0, 0, 0xee, 0xb0, 63, 0xee, 0xb0, 0, 0xee, 0xc3, // RGB = green.
                ],
            )
            .await;
            for plane in 0..4 {
                assert!(std::ptr::eq(
                    red.display.plane(plane),
                    green.display.plane(plane)
                ));
            }
            assert_image(
                &gpu.render(&green, size).await,
                &expected_image(size, active, GREEN, marked_pixel),
                size[0],
            );
            retained.push((red, size, expected));
        }
        drop(dos);
        for (capture, size, expected) in retained.into_iter().rev() {
            assert_image(&gpu.render(&capture, size).await, &expected, size[0]);
        }
    });
}

#[test]
fn guest_raster_skew_panning_and_split_keep_border_coordinates_independent() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let mut gpu = Gpu::new().await;
        let mut program = vec![0xb8, 0x13, 0, 0xcd, 0x10];
        program.extend_from_slice(RED_BORDER);
        program.extend_from_slice(&[0xba, 0xd4, 3, 0xb8, 0x11, 0, 0xef]);
        // Ten clocks by ten lines. A one-clock display skew puts the eight-pixel
        // picture at x=12 inside a 24-pixel image. Blanking adds two lines above it.
        for (index, value) in [
            (0, 5),
            (1, 1),
            (2, 3),
            (3, 0xa7),
            (5, 0),
            (6, 8),
            (7, 0),
            (8, 0),
            (9, 0),
            (12, 0),
            (13, 0x10),
            (0x10, 7),
            (0x11, 9),
            (0x12, 3),
            (0x13, 1),
            (0x14, 0x40), // Doubleword addresses match the chain-4 guest writes.
            (0x15, 5),
            (0x16, 7),
            (0x17, 0xe3),
            (0x18, 1),
        ] {
            program.extend_from_slice(&[0xb8, index, value, 0xef]);
        }
        program.extend_from_slice(&[
            0xba, 0xda, 3, 0xec, 0xba, 0xc0, 3, // Reset AC access phase.
            0xb0, 0x30, 0xee, 0xb0, 0x61, 0xee, // Reset panning below the split.
            0xb0, 0x33, 0xee, 0xb0, 2, 0xee, // One indexed pixel of panning.
            0xb0, 0x20, 0xee, // Display enabled.
            0xb8, 0, 0xa0, 0x8e, 0xc0, // ES = A000.
        ]);
        for (start, colors) in [
            (
                0u8,
                &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 1][..],
            ),
            (
                0x40,
                &[4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 1, 2, 3, 4, 5][..],
            ),
        ] {
            for (offset, &color) in colors.iter().enumerate() {
                program.extend_from_slice(&[0x26, 0xc6, 0x06, start + offset as u8, 0, color]);
            }
        }
        program.push(0xc3);
        let split = run(&mut dos, &program).await;
        assert_eq!(split.display.registers.crtc.display_start, 0x10);
        let rows = [
            [5, 6, 7, 8, 9, 10, 11, 12],
            [13, 14, 15, 1, 2, 3, 4, 5],
            [1, 2, 3, 4, 5, 6, 7, 8],
            [9, 10, 11, 12, 13, 14, 15, 1],
        ];
        let expected = expected_image([24, 8], [12..20, 2..6], RED, |x, y| {
            COLORS[rows[y as usize][x as usize]]
        });
        assert_image(&gpu.render(&split, [24, 8]).await, &expected, 24);

        // Only registers change: remove display skew and keep panning at the split.
        let panned = run(
            &mut dos,
            &[
                0xba, 0xd4, 3, 0xb8, 3, 0x87, 0xef, // Display-enable skew = 0.
                0xba, 0xda, 3, 0xec, 0xba, 0xc0, 3, // Reset AC access phase.
                0xb0, 0x30, 0xee, 0xb0, 0x41, 0xee, // Preserve panning below the split.
                0xb0, 0x20, 0xee, 0xc3, // Display enabled; return to DOS.
            ],
        )
        .await;
        let panned_rows = [
            rows[0],
            rows[1],
            [2, 3, 4, 5, 6, 7, 8, 9],
            [10, 11, 12, 13, 14, 15, 1, 0],
        ];
        assert_image(
            &gpu.render(&panned, [24, 8]).await,
            &expected_image([24, 8], [8..16, 2..6], RED, |x, y| {
                COLORS[panned_rows[y as usize][x as usize]]
            }),
            24,
        );
        let text = run(&mut dos, TEXT_WITH_RED_BLOCK).await;
        drop(dos);
        assert_image(
            &gpu.render(&text, [738, 414]).await,
            &expected_image([738, 414], [9..729, 7..407], BLACK, |x, y| {
                if x < 9 && y < 16 {
                    COLORS[4]
                } else {
                    BLACK
                }
            }),
            738,
        );
        assert_image(&gpu.render(&split, [24, 8]).await, &expected, 24);
    });
}
