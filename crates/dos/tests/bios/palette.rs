use super::video::{video, PROGRAM};
use crate::common;
use relocade_x86::{Segment, SegmentRegister};

#[test]
fn attribute_services_handle_both_ports_and_preserve_display_enable() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (miscellaneous, status) in [(0x43, 0x3da), (0x42, 0x3ba)] {
            for address in [0x25, 5] {
                for data_next in [false, true] {
                    {
                        let context = dos.cpu().context();
                        context.io.write_port(0x3c2, 1, miscellaneous);
                        context.io.read_port(status, 1);
                        context.io.write_port(0x3c0, 1, address);
                        if !data_next {
                            context.io.read_port(status, 1);
                        }
                    }
                    video(&mut dos, 0x1000, 0xff02, 0, 0);
                    let read = video(&mut dos, 0x1007, 0xa502, 0, 0);
                    assert_eq!(read.registers.ebx as u16, 0x3f02);
                    let capture = dos.vga().borrow().snapshot();
                    assert_eq!(capture.registers.attribute.values[2], 63);
                    assert_eq!(u32::from(capture.registers.attribute.address()), address);
                    video(&mut dos, 0x1001, 0x2a00, 0, 0);
                    assert_eq!(
                        video(&mut dos, 0x1008, 0x005a, 0, 0).registers.ebx as u16,
                        0x2a5a
                    );
                }
            }
        }
    });
}

#[test]
fn attribute_blocks_wrap_es_offsets_and_keep_overscan_separate() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let values = [
            63, 62, 61, 60, 59, 58, 57, 56, 55, 54, 53, 52, 51, 50, 49, 48, 42,
        ];
        {
            let mut context = dos.cpu().context();
            context
                .cpu
                .set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x5000));
            context.memory.write(0x5fff8, &values[..8]).unwrap();
            context.memory.write(0x50000, &values[8..]).unwrap();
            context.memory.write(0x60000, &[0xa5; 17]).unwrap();
        }
        let before = dos.vga().borrow().snapshot();
        video(&mut dos, 0x1002, 0, 0, 0xfff8);
        let capture = dos.vga().borrow().snapshot();
        assert_eq!(&capture.registers.attribute.values[..16], &values[..16]);
        assert_eq!(capture.registers.attribute.values[0x11], 42);
        assert_eq!(
            capture.registers.attribute.values[0x10],
            before.registers.attribute.values[0x10]
        );
        video(&mut dos, 0x1009, 0, 0, 0xfffc);
        let context = dos.cpu().context();
        let mut tail = [0; 4];
        let mut head = [0; 13];
        let mut guard = [0; 17];
        context.memory.read(0x5fffc, &mut tail).unwrap();
        context.memory.read(0x50000, &mut head).unwrap();
        context.memory.read(0x60000, &mut guard).unwrap();
        assert_eq!(tail, values[..4]);
        assert_eq!(head, values[4..]);
        assert_eq!(guard, [0xa5; 17]);
    });
}

#[test]
fn blink_paging_and_dac_mask_services_update_only_their_fields() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        // AX=1000h also accepts the extended Attribute Controller registers.
        video(&mut dos, 0x1000, 0x2510, 0, 0);
        video(&mut dos, 0x1003, 1, 0, 0);
        assert_eq!(
            video(&mut dos, 0x1007, 0x10, 0, 0).registers.ebx as u16,
            0x2d10
        );
        video(&mut dos, 0x1003, 0, 0, 0);
        assert_eq!(
            video(&mut dos, 0x1007, 0x10, 0, 0).registers.ebx as u16,
            0x2510
        );
        video(&mut dos, 0x1013, 0x0100, 0, 0); // Sixteen pages of sixteen colors.
        video(&mut dos, 0x1013, 0x0b01, 0, 0); // Page eleven.
        assert_eq!(
            video(&mut dos, 0x101a, 0, 0, 0).registers.ebx as u16,
            0x0b01
        );
        assert_eq!(
            video(&mut dos, 0x1007, 0x10, 0, 0).registers.ebx as u16,
            0xa510
        );
        video(&mut dos, 0x1013, 0, 0, 0); // Four pages of sixty-four colors.
        assert_eq!(
            video(&mut dos, 0x101a, 0, 0, 0).registers.ebx as u16,
            0x0200
        );
        video(&mut dos, 0x1013, 0x0101, 0, 0); // Page one in the four-page layout.
        assert_eq!(
            video(&mut dos, 0x101a, 0, 0, 0).registers.ebx as u16,
            0x0100
        );
        assert_eq!(
            video(&mut dos, 0x1007, 0x14, 0, 0).registers.ebx as u16,
            0x0414
        );
        video(&mut dos, 0x1018, 0x007f, 0, 0);
        assert_eq!(
            video(&mut dos, 0x1019, 0xa500, 0, 0).registers.ebx as u16,
            0xa57f
        );
        video(&mut dos, 0x0013, 0, 0, 0);
        let before = dos.vga().borrow().snapshot();
        for (ax, bx) in [(0x1013, 0), (0x1003, 2), (0x1000, 0x15), (0x10ff, 0)] {
            assert_eq!(
                video(&mut dos, ax, bx, 0, 0).registers.eax as u16,
                0x8600 | (ax & 255)
            );
        }
        assert_eq!(before.registers, dos.vga().borrow().snapshot().registers);
    });
}

#[test]
fn dac_calls_mask_components_and_wrap_palette_and_segment_addresses() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x1010, 255, 0x4283, 0xff5a);
        let single = video(&mut dos, 0x1015, 0xa5ff, 0, 0x005a);
        assert_eq!(single.registers.ebx as u16, 0xa5ff);
        assert_eq!(single.registers.ecx as u16, 0x0203);
        assert_eq!(single.registers.edx as u16, 0x3f5a);
        let values = [63, 0, 21, 0, 63, 42, 7, 9, 11];
        {
            let mut context = dos.cpu().context();
            context
                .cpu
                .set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x5000));
            context.memory.write_u8(0x5ffff, values[0]).unwrap();
            context.memory.write(0x50000, &values[1..]).unwrap();
            context.memory.write(0x60000, &[0xa5; 9]).unwrap();
        }
        video(&mut dos, 0x1012, 255, 3, 0xffff);
        let capture = dos.vga().borrow().snapshot();
        assert_eq!(capture.registers.dac.colors[255], [63, 0, 21]);
        assert_eq!(capture.registers.dac.colors[0], [0, 63, 42]);
        assert_eq!(capture.registers.dac.colors[1], [7, 9, 11]);
        video(&mut dos, 0x1017, 255, 3, 0xfffe);
        {
            let context = dos.cpu().context();
            let mut tail = [0; 2];
            let mut head = [0; 7];
            let mut guard = [0; 9];
            context.memory.read(0x5fffe, &mut tail).unwrap();
            context.memory.read(0x50000, &mut head).unwrap();
            context.memory.read(0x60000, &mut guard).unwrap();
            assert_eq!(tail, values[..2]);
            assert_eq!(head, values[2..]);
            assert_eq!(guard, [0xa5; 9]);
        }
        // Reads and writes of zero colors preserve even DAC streaming state.
        let before = dos.vga().borrow().snapshot();
        for ax in [0x1012, 0x1017] {
            video(&mut dos, ax, 42, 0, 0xffff);
        }
        assert_eq!(
            before.registers.dac,
            dos.vga().borrow().snapshot().registers.dac
        );
        // A later palette write cannot alter a retained capture.
        video(&mut dos, 0x1010, 0, 0, 0);
        assert_eq!(capture.registers.dac.colors[0], [0, 63, 42]);
    });
}
