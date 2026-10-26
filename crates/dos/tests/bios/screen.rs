use crate::common;
use relocade_vga::{TextCell, TextCursor, TextScreen};
use relocade_x86::ExecutionExit;

#[test]
fn guest_memory_and_ports_drive_vga_independently_of_bios_bookkeeping() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let memory = dos.cpu().memory();
        let vga = dos.vga();
        let mut view = memory.view();
        view.write_backing(0xb8000, &[0x99, 0xee]).unwrap();
        let screen = vga.screen();
        assert_eq!(screen.start_address, 0);
        assert_eq!(
            screen.cursor,
            TextCursor {
                address: 0,
                start_scan_line: 6,
                end_scan_line: 7,
            }
        );
        assert_eq!(
            screen.cells,
            [TextCell {
                character: b' ',
                attribute: 7
            }; 80 * 25]
        );
        let mut geometry = [0; 6];
        dos.cpu()
            .context()
            .memory
            .read(0x449, &mut geometry)
            .unwrap();
        assert_eq!(geometry, [3, 80, 0, 0, 0x10, 0]);

        dos.launch_com(
            &[
                0xb8, 0x00, 0xb8, // MOV AX,B800.
                0x8e, 0xc0, // MOV ES,AX.
                0x26, 0xc7, 0x06, 0, 0, b'A', 0x1e, // MOV word ES:[0],1E41.
                0xeb, 0xfe,
            ],
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            pollster::block_on(dos.cpu().run(10)),
            ExecutionExit::BudgetExhausted
        );
        let screen = vga.screen();
        assert_eq!(
            screen.cells[0],
            TextCell {
                character: b'A',
                attribute: 0x1e
            }
        );
        assert_eq!(
            screen.cells[1],
            TextCell {
                character: b' ',
                attribute: 7
            }
        );
        assert!(common::output(dos.stdout()).is_empty());
        let mut backing = [0; 2];
        view.read_backing(0xb8000, &mut backing).unwrap();
        assert_eq!(backing, [0x99, 0xee]);

        // BIOS bookkeeping alone does not program the hardware.
        {
            let mut machine = dos.cpu().context();
            let memory = &mut machine.memory;
            memory.write(0x462, &[3]).unwrap();
            memory.write(0x456, &[79, 24]).unwrap();
            memory.write(0xbff9e, &[b'Z', 0x2f]).unwrap();
        }
        assert_eq!(vga.screen(), screen);
        {
            let mut context = dos.cpu().context();
            context
                .memory
                .write(
                    0x10100,
                    &[
                        0xba, 0xd4, 0x03, // MOV DX,03D4.
                        0xb8, 0x0c, 0x38, 0xef, // CRTC start high = 38.
                        0xb8, 0x0d, 0, 0xef, // CRTC start low = 00 (page 7).
                        0xb8, 0x0e, 0x3f, 0xef, // Cursor high = 3F.
                        0xb8, 0x0f, 0xcf, 0xef, // Cursor low = CF (last visible cell).
                        0xeb, 0xfe,
                    ],
                )
                .unwrap();
            context.cpu.set_eip(0x100);
        }
        assert_eq!(dos.cpu().run(30).await, ExecutionExit::BudgetExhausted);
        let page = vga.screen();
        assert_eq!(page.start_address, 0x3800);
        assert_eq!(page.cursor.address, 0x3fcf);
        assert_eq!(view.read_u8(0x462).unwrap(), 3);
        assert_eq!(view.read_u16(0x456).unwrap(), 0x184f);
        assert_eq!(
            page.cells[TextScreen::COLUMNS * TextScreen::ROWS - 1],
            TextCell {
                character: b'Z',
                attribute: 0x2f
            }
        );
        assert_eq!(screen.cells[0].character, b'A'); // Snapshots remain owned.
        drop(dos);
        assert_eq!(vga.screen(), page); // The device outlives execution.
    });
}
