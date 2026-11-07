use super::super::video::{video, PROGRAM};
use crate::common;
use relocade_machine::FarPointer;
use relocade_x86::{Segment, SegmentRegister};

#[test]
fn dac_format_reports_fixed_width_without_disturbing_palette_transfers() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for mode in [3, 0x101, 0x103, 0x105] {
            video(&mut dos, 0x4f02, mode, 0, 0);
            {
                let context = dos.cpu().context();
                context.io.write_port(0x3c8, 1, 17);
                context.io.write_port(0x3c9, 1, 0xfe); // Leave the transfer at green.
                context.io.write_port(0x1ce, 2, 0x1234);
                context.cpu.set_si(0x2345);
                context.cpu.set_di(0x3456);
                context.cpu.set_bp(0x4567);
            }
            let registers = dos.vga().borrow().registers().clone();
            for (request, status, result) in [
                (0x0001, 0x4f, 0x0601),
                (0xff01, 0x4f, 0x0601),
                (0x0600, 0x4f, 0x0600),
                (0x0700, 0x4f, 0x0600),
                (0x0800, 0x4f, 0x0600),
                (0xff00, 0x4f, 0x0600),
                (0x0000, 0x14f, 0x0000),
                (0x0500, 0x14f, 0x0500),
                (0x0602, 0x14f, 0x0602),
                (0x0680, 0x14f, 0x0680),
            ] {
                let cpu = video(&mut dos, 0x4f08, request, 0x5678, 0x6789);
                assert_eq!(cpu.registers.eax as u16, status);
                assert_eq!(cpu.registers.ebx as u16, result);
                assert_eq!(cpu.registers.ecx as u16, 0x5678);
                assert_eq!(cpu.registers.edx as u16, 0x6789);
                assert_eq!(cpu.registers.esi as u16, 0x2345);
                assert_eq!(cpu.registers.edi as u16, 0x3456);
                assert_eq!(cpu.registers.ebp as u16, 0x4567);
                assert_eq!(dos.vga().borrow().registers(), &registers);
                assert_eq!(dos.cpu().context().io.read_port(0x1ce, 2), 0x1234);
            }
        }
        dos.cpu().context().io.write_port(0x46e8, 1, 0);
        assert_eq!(video(&mut dos, 0x4f08, 1, 0, 0).registers.eax as u16, 0x14f);
    });
}

#[test]
fn direct_color_modes_preserve_the_dac_and_reject_format_control() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        {
            let context = dos.cpu().context();
            context.io.write_port(0x3c8, 1, 17);
            context.io.write_port(0x3c9, 1, 42); // Leave the transfer at green.
            context.io.write_port(0x3c6, 1, 0x5a);
        }
        let dac = dos.vga().borrow().dac_state();
        for mode in [0x111, 0x114, 0x117, 0x142, 0x143, 0x144] {
            assert_eq!(video(&mut dos, 0x4f02, mode, 0, 0).registers.eax, 0x4f);
            assert_eq!(dos.vga().borrow().dac_state(), dac);
            dos.cpu().context().io.write_port(0x1ce, 2, 0x1234);
            for request in [1, 0x0600, 0x0800] {
                let result = video(&mut dos, 0x4f08, request, 0x5678, 0x6789).registers;
                assert_eq!(
                    [result.eax, result.ebx, result.ecx, result.edx],
                    [0x34f, u32::from(request), 0x5678, 0x6789]
                );
                assert_eq!(dos.vga().borrow().dac_state(), dac);
                assert_eq!(dos.cpu().context().io.read_port(0x1ce, 2), 0x1234);
            }
        }
    });
}

#[test]
fn packed_modes_share_legacy_palette_colors_mask_and_wrapping_buffers() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x4f02, 0x103, 0, 0);
        video(&mut dos, 0x4f05, 0, 0, 7);
        let source = FarPointer {
            segment: 0x5000,
            offset: 0xfffe,
        };
        {
            let mut context = dos.cpu().context();
            context.cpu.set_segment(
                Segment::Es,
                SegmentRegister::real_mode(Segment::Es, source.segment),
            );
            context
                .memory
                .write_wrapping(source, &[127, 64, 63, 32, 33, 34])
                .unwrap();
            context.memory.write_u8(0xa0010, 0x7b).unwrap();
            context.memory.write(0x50200, &[0xa5; 8]).unwrap();
        }
        video(&mut dos, 0x1012, 255, 2, source.offset);
        video(&mut dos, 0x1017, 255, 2, 0x201);
        let mut colors = [0; 8];
        dos.cpu()
            .context()
            .memory
            .read(0x50200, &mut colors)
            .unwrap();
        assert_eq!(colors, [0xa5, 63, 0, 63, 32, 33, 34, 0xa5]);

        video(&mut dos, 0x1010, 17, 0x090a, 0x0800);
        let color = video(&mut dos, 0x1015, 17, 0, 0);
        assert_eq!(color.registers.ecx as u16, 0x090a);
        assert_eq!(color.registers.edx as u16, 0x0800);
        video(&mut dos, 0x1018, 0xa5, 0, 0);
        assert_eq!(video(&mut dos, 0x1019, 0, 0, 0).registers.ebx as u16, 0xa5);
        let capture = dos.vga().borrow().snapshot();
        assert_eq!(capture.dac.colors[255], [63, 0, 63]);
        assert_eq!(capture.dac.colors[0], [32, 33, 34]);
        assert_eq!(capture.dac.colors[17], [8, 9, 10]);
        assert_eq!(capture.dac.mask, 0xa5);
        assert_eq!(video(&mut dos, 0x4f05, 0x100, 0, 0).registers.edx as u16, 7);
        assert_eq!(dos.cpu().context().memory.read_u8(0xa0010).unwrap(), 0x7b);
    });
}
