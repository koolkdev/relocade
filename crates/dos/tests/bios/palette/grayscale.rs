use crate::{
    common,
    video::{video, PROGRAM},
};

#[test]
fn grayscale_uses_ibm_rounding_and_wraps_only_the_requested_dac_range() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let colors: [[u8; 3]; 8] = [
            [63, 0, 0],
            [0, 63, 0],
            [0, 0, 41],
            [63, 63, 63],
            [5, 0, 0],
            [0, 50, 0],
            [0, 0, 50],
            [0, 0, 0],
        ];
        {
            let context = dos.cpu().context();
            context.io.write_port(0x3c8, 1, 252);
            for color in colors {
                for component in color {
                    context.io.write_port(0x3c9, 1, u32::from(component));
                }
            }
            context.io.write_port(0x3c6, 1, 0x5a);
        }
        let before = dos.vga().borrow().snapshot();
        let result = video(&mut dos, 0x101b, 0xa5fc, 8, 0x1234);
        assert_eq!(result.registers.eax as u16, 0x101b);
        assert_eq!(result.registers.ebx as u16, 0xa5fc);
        assert_eq!(result.registers.ecx as u16, 8);
        assert_eq!(result.registers.edx as u16, 0x1234);
        let after = dos.vga().borrow().snapshot();
        let mut expected = before.registers.dac.colors;
        // Pure red=5 and green/blue=50 round below the decimal half boundary.
        for (index, intensity) in [
            (252, 19),
            (253, 37),
            (254, 5),
            (255, 63),
            (0, 1),
            (1, 29),
            (2, 5),
            (3, 0),
        ] {
            expected[index] = [intensity; 3];
        }
        assert_eq!(after.registers.dac.colors, expected);
        assert_eq!(after.registers.dac.mask, 0x5a);
        assert_eq!(after.registers.dac.address(), 4);
        let mut registers = after.registers.clone();
        registers.dac = before.registers.dac.clone();
        assert_eq!(registers, before.registers);
        for plane in 0..4 {
            assert!(std::ptr::eq(after.plane(plane), before.plane(plane)));
        }
        // Retained captures keep the original colors after the port writes.
        assert_eq!(before.registers.dac.colors[252], [63, 0, 0]);

        // A full palette uses the high byte of CX, not an eight-bit count.
        {
            let context = dos.cpu().context();
            context.io.write_port(0x3c8, 1, 0);
            for _ in 0..256 {
                for component in [5, 0, 0] {
                    context.io.write_port(0x3c9, 1, component);
                }
            }
        }
        video(&mut dos, 0x101b, 0, 256, 0);
        let full = dos.vga().borrow().snapshot();
        assert_eq!(full.registers.dac.colors, [[1; 3]; 256]);
        assert_eq!(full.registers.dac.address(), 0);
    });
}

#[test]
fn zero_count_grayscale_preserves_unfinished_dac_reads_and_writes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for reading in [false, true] {
            {
                let context = dos.cpu().context();
                context
                    .io
                    .write_port(if reading { 0x3c7 } else { 0x3c8 }, 1, 42);
                if reading {
                    context.io.read_port(0x3c9, 1);
                } else {
                    context.io.write_port(0x3c9, 1, 17);
                }
            }
            let before = dos.vga().borrow().snapshot();
            video(&mut dos, 0x101b, 255, 0, 0);
            assert_eq!(dos.vga().borrow().snapshot().registers, before.registers);
        }
    });
}
