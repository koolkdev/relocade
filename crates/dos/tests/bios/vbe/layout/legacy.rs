use super::{request, scanline, video, PROGRAM};
use crate::common::{self, VideoInspection};
use relocade_vga::FramebufferAccess;

#[test]
fn text_scanlines_use_live_character_dimensions_and_aperture_capacity() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        scanline(&mut dos, 1, 0, [0x4f, 160, 720, 3264]);
        scanline(&mut dos, 0, 721, [0x4f, 164, 738, 3184]);
        scanline(&mut dos, 3, 0, [0x4f, 1020, 4590, 512]);
        scanline(&mut dos, 1, 0, [0x4f, 164, 738, 3184]);
        dos.cpu()
            .context()
            .memory
            .write_u16(0xb8000 + 164, 0x1f41)
            .unwrap();
        assert_eq!(
            dos.vga()
                .borrow_mut()
                .framebuffer()
                .text_cell(1, 0)
                .unwrap()
                .value(),
            0x1f41
        );
        // Logical width does not change the physical grid or BIOS mode information.
        assert_eq!(
            video(&mut dos, 0x0f00, 0, 0, 0).registers.eax as u16,
            0x5003
        );
        assert_eq!(dos.cpu().context().memory.read_u16(0x44a).unwrap(), 80);

        // Guest CRTC writes and a BIOS font change are visible to the same query.
        dos.cpu().context().io.write_port(0x3d4, 2, 0x3213);
        scanline(&mut dos, 1, 0, [0x4f, 200, 900, 2608]);
        video(&mut dos, 3, 0, 0, 0);
        video(&mut dos, 0x1112, 0, 0, 0); // Eight-row glyphs, fifty visible text rows.
        dos.cpu().context().io.write_port(0x3c4, 2, 0x0101); // Eight-dot cells.
        scanline(&mut dos, 1, 0, [0x4f, 160, 640, 1632]);
        scanline(&mut dos, 2, 161, [0x4f, 164, 656, 1592]);
        scanline(&mut dos, 3, 0, [0x4f, 652, 2608, 400]);
    });
}

#[test]
fn planar_scanlines_round_bytes_and_preserve_the_programmed_viewport() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x12, 0, 0, 0);
        scanline(&mut dos, 1, 0, [0x4f, 80, 640, 819]);
        {
            let context = dos.cpu().context();
            // Pixel (11,20): start counter 1601 plus three pixels of fine pan.
            for value in [0x060c, 0x410d] {
                context.io.write_port(0x3d4, 2, value);
            }
            context.io.read_port(0x3da, 1);
            context.io.write_port(0x3c0, 1, 0x33);
            context.io.write_port(0x3c0, 1, 3);
        }
        scanline(&mut dos, 0, 641, [0x4f, 82, 656, 799]);
        let registers = dos.vga().borrow().registers().clone();
        assert_eq!(&registers.crtc.registers.values[0x0c..0x0e], &[6, 0x69]);
        assert_eq!(registers.attribute.values[0x13], 3);
        scanline(&mut dos, 2, 83, [0x4f, 84, 672, 780]);
        scanline(&mut dos, 3, 0, [0x4f, 136, 1088, 481]);
        let before = dos.vga().borrow().core_state();
        scanline(&mut dos, 0, 640, [0x14f, 0, 640, 0xabcd]);
        scanline(&mut dos, 2, 137, [0x24f, 2, 137, 0xabcd]);
        scanline(&mut dos, 4, 0, [0x14f, 4, 0, 0xabcd]);
        assert_eq!(dos.vga().borrow().core_state(), before);

        video(&mut dos, 0x12, 0, 0, 0);
        scanline(&mut dos, 2, 99, [0x4f, 100, 800, 655]);
        {
            let mut context = dos.cpu().context();
            context.io.write_port(0x3c4, 2, 0x0102); // Plane zero.
            context.memory.write_u8(0xa0000 + 100, 0x80).unwrap();
        }
        assert_eq!(
            dos.vga()
                .borrow_mut()
                .framebuffer()
                .pixel(0, 1)
                .unwrap()
                .value(),
            1
        );
    });
}

#[test]
fn chained_scanlines_are_limited_to_the_legacy_aperture() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x13, 0, 0, 0);
        scanline(&mut dos, 1, 0, [0x4f, 320, 320, 204]);
        scanline(&mut dos, 3, 0, [0x4f, 320, 320, 204]);
        scanline(&mut dos, 0, 319, [0x4f, 320, 320, 204]);
        scanline(&mut dos, 2, 321, [0x24f, 2, 321, 0xabcd]);
        // A guest-shortened raster can accommodate wider logical rows.
        dos.cpu().context().io.write_port(0x3d4, 2, 0x6712); // 360 scanlines, 180 pixels.
        scanline(&mut dos, 0, 321, [0x4f, 328, 328, 199]);
        dos.cpu()
            .context()
            .memory
            .write_u8(0xa0000 + 328, 0x5a)
            .unwrap();
        assert_eq!(
            dos.vga()
                .borrow_mut()
                .framebuffer()
                .pixel(0, 1)
                .unwrap()
                .value(),
            0x5a
        );

        // Interleaved CGA rows cannot be described by a single linear byte pitch.
        video(&mut dos, 4, 0, 0, 0);
        assert_eq!(request(&mut dos, 6, 1, 0, 0).registers.eax, 0x14f);
        video(&mut dos, 0x12, 0, 0, 0);
        dos.cpu().context().io.write_port(0x3d4, 2, 0x0009);
        dos.cpu().context().io.write_port(0x3d4, 2, 0x0018); // Split at line 257.
        assert_eq!(request(&mut dos, 6, 1, 0, 0).registers.eax, 0x14f);
    });
}
