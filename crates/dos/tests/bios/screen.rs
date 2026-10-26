use crate::common;
use relocade_bios::{TextCell, TextCursor, TextScreen};
use relocade_x86::ExecutionExit;

#[test]
fn text_screen_reads_the_same_memory_written_by_guest_instructions() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let memory = dos.cpu().memory();
        let view = memory.view();
        let screen = TextScreen::read(&view);
        assert_eq!(screen.page, 0);
        assert_eq!(
            screen.cursor,
            TextCursor {
                row: 0,
                column: 0,
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

        dos.load_com(
            &[
                0xb8, 0x00, 0xb8, // MOV AX,B800.
                0x8e, 0xc0, // MOV ES,AX.
                0x26, 0xc7, 0x06, 0, 0, b'A', 0x1e, // MOV word ES:[0],1E41.
                0xeb, 0xfe,
            ],
            Default::default(),
        )
        .unwrap();
        assert_eq!(dos.cpu().run(10).unwrap(), ExecutionExit::BudgetExhausted);
        let screen = TextScreen::read(&view);
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

        // A different active page and its cursor are read from the BIOS data area.
        {
            let mut machine = dos.cpu().context();
            let memory = &mut machine.memory;
            memory.write(0x462, &[3]).unwrap();
            memory.write(0x456, &[79, 24]).unwrap();
            memory.write(0xbbf9e, &[b'Z', 0x2f]).unwrap();
        }
        let page = TextScreen::read(&view);
        assert_eq!(page.page, 3);
        assert_eq!(page.cursor.row, 24);
        assert_eq!(page.cursor.column, 79);
        assert_eq!(
            page.cells[TextScreen::COLUMNS * TextScreen::ROWS - 1],
            TextCell {
                character: b'Z',
                attribute: 0x2f
            }
        );
        assert_eq!(screen.cells[0].character, b'A'); // Snapshots remain owned.
    });
}
