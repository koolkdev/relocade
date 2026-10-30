use super::{common, run, Gpu};

const PALETTE_ONE: [[u8; 4]; 4] = [
    [0, 0, 0, 255],
    [85, 255, 255, 255],
    [255, 85, 255, 255],
    [255, 255, 255, 255],
];
const PALETTE_ZERO: [[u8; 4]; 4] = [
    [0, 0, 0, 255],
    [85, 255, 85, 255],
    [255, 85, 85, 255],
    [255, 255, 85, 255],
];

fn assert_packed_image(image: &[u8], memory: &[u8], width: usize, colors: &[[u8; 4]]) {
    assert_eq!(image.len(), width * 200 * 4);
    for (offset, pixel) in image.as_chunks::<4>().0.iter().enumerate() {
        let x = offset % width;
        let y = offset / width;
        // CPU-visible CGA storage: alternating 8-KiB banks, 80 bytes per row.
        let row = (y % 2) * 8192 + (y / 2) * 80;
        let color = if width == 320 {
            (memory[row + x / 4] >> (6 - (x % 4) * 2)) & 3
        } else {
            (memory[row + x / 8] >> (7 - x % 8)) & 1
        };
        assert_eq!(*pixel, colors[usize::from(color)], "pixel ({x}, {y})");
    }
}

#[test]
fn guest_cga_modes_decode_banked_writes_and_recolor_retained_captures() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let mut gpu = Gpu::new().await;
        let memory: Vec<u8> = (0..16384)
            .map(|offset| ((offset * 5) ^ (offset >> 6) ^ (offset >> 13)) as u8)
            .collect();
        let mut retained = Vec::new();
        for mode in [4, 5, 6] {
            let mut program = vec![
                0xb8, mode, 0, 0xcd, 0x10, // BIOS mode set.
                0xb8, 0, 0xb8, 0x8e, 0xc0, // ES = B800.
                0x31, 0xff, 0xbe, 0, 3, 0xb9, 0, 0x40, // DI = 0, SI = 300h, CX = 16 KiB.
                0xfc, 0xf3, 0xa4, 0xc3, // Copy packed bytes through guest MMIO.
            ];
            program.resize(0x200, 0);
            program.extend_from_slice(&memory);
            let capture = run(&mut dos, &program).await;
            let width = if mode == 6 { 640 } else { 320 };
            let size = [width, 200];
            let colors = if mode == 6 {
                [
                    PALETTE_ONE[0],
                    PALETTE_ONE[3],
                    PALETTE_ONE[0],
                    PALETTE_ONE[3],
                ]
            } else {
                PALETTE_ONE
            };
            let image = gpu.render(&capture, size).await;
            assert_packed_image(&image, &memory, width as usize, &colors);
            retained.push((capture.clone(), size, image));
            if mode == 4 {
                let recolored = run(
                    &mut dos,
                    &[0xb8, 0, 0x0b, 0xbb, 0, 1, 0xcd, 0x10, 0xc3], // AH=0Bh, BH=1, BL=0.
                )
                .await;
                for plane in 0..4 {
                    assert!(std::ptr::eq(
                        capture.display.plane(plane),
                        recolored.display.plane(plane)
                    ));
                }
                let image = gpu.render(&recolored, size).await;
                assert_packed_image(&image, &memory, 320, &PALETTE_ZERO);
                retained.push((recolored, size, image));
            }
        }
        drop(dos);
        for (capture, size, expected) in retained {
            assert_eq!(gpu.render(&capture, size).await, expected);
        }
    });
}
