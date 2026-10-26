use crate::common;
use relocade_bios::TextCell;
use relocade_dos::Dos;
use relocade_x86::{CpuState, ExecutionExit};

const PROGRAM: &[u8] = &[0xcd, 0x10, 0xeb, 0xfe];

fn video(dos: &mut Dos, ax: u16, bx: u16, cx: u16, dx: u16) -> CpuState {
    {
        let mut machine = dos.cpu().context();
        let cpu = &mut machine.cpu;
        cpu.set_eip(0x100);
        cpu.set_ax(ax);
        cpu.set_bx(bx);
        cpu.set_cx(cx);
        cpu.set_dx(dx);
        cpu.set_eflags(0x643);
    }
    assert_eq!(dos.cpu().run(15).unwrap(), ExecutionExit::BudgetExhausted);
    let cpu = dos.cpu().context().cpu.snapshot();
    assert_eq!(cpu.registers.esp as u16, 0xfffe);
    assert_eq!(cpu.flags.eflags(), 0x643);
    cpu
}

#[test]
fn video_services_share_pages_cursors_and_cell_attributes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x0100, 0, 0x2007, 0); // Hide cursor.
        video(&mut dos, 0x0200, 0x0300, 0, 0x0204); // Page 3, row 2, column 4.
        video(&mut dos, 0x0958, 0x032e, 3, 0); // Three yellow-on-green X cells.
        let cursor = video(&mut dos, 0x0300, 0x0300, 0, 0);
        assert_eq!(cursor.registers.ecx as u16, 0x2007);
        assert_eq!(cursor.registers.edx as u16, 0x0204); // AH=09h preserves cursor.
        assert_eq!(
            video(&mut dos, 0x0800, 0x0300, 0, 0).registers.eax as u16,
            0x2e58
        );
        video(&mut dos, 0x0a59, 0x03ff, 1, 0); // AH=0Ah preserves the attribute.
        video(&mut dos, 0x0503, 0, 0, 0);
        let mode = video(&mut dos, 0x0f00, 0x00a5, 0, 0);
        assert_eq!(mode.registers.eax as u16, 0x5003);
        assert_eq!(mode.registers.ebx as u16, 0x03a5);
        let screen = dos.screen();
        assert_eq!(screen.page, 3);
        assert_eq!(screen.cursor.start_scan_line, 0x20);
        assert_eq!(
            screen.cells[164],
            TextCell {
                character: b'Y',
                attribute: 0x2e
            }
        );
        assert_eq!(
            screen.cells[165],
            TextCell {
                character: b'X',
                attribute: 0x2e
            }
        );
        let mut page_offset = [0; 2];
        dos.cpu()
            .context()
            .memory
            .read(0x44e, &mut page_offset)
            .unwrap();
        assert_eq!(page_offset, [0, 0x30]);
        // Teletype writes the active page, independently of incoming BH.
        video(&mut dos, 0x0e5a, 0xff00, 0, 0);
        assert_eq!(dos.screen().cells[164].character, b'Z');
        assert_eq!(dos.screen().cursor.column, 5);

        // Mode reset returns AL=30h and clears pages and cursors.
        assert_eq!(
            video(&mut dos, 0x0003, 0, 0, 0).registers.eax as u16,
            0x0030
        );
        let screen = dos.screen();
        assert_eq!(screen.page, 0);
        assert_eq!(screen.cursor.row, 0);
        assert_eq!(screen.cursor.column, 0);
        video(&mut dos, 0x0503, 0, 0, 0);
        assert_eq!(
            dos.screen().cells[164],
            TextCell {
                character: b' ',
                attribute: 7
            }
        );
        assert!(common::output(dos.stdout()).is_empty());
    });
}

#[test]
fn repeated_writes_cross_page_padding_without_moving_the_cursor() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x0200, 0, 0, 0x184f);
        // 4000 visible bytes occupy a 4096-byte page. Fifty cells starting at
        // the last visible cell include the padding and the next page's first cell.
        video(&mut dos, 0x0951, 0x001e, 50, 0);
        for address in [0xb8f9e, 0xb8fa0, 0xb8ffe, 0xb9000] {
            let mut cell = [0; 2];
            dos.cpu().context().memory.read(address, &mut cell).unwrap();
            assert_eq!(cell, [b'Q', 0x1e]);
        }
        let cursor = video(&mut dos, 0x0300, 0, 0, 0);
        assert_eq!(cursor.registers.edx as u16, 0x184f);
        video(&mut dos, 0x0a4b, 0, 50, 0);
        let mut cell = [0; 2];
        dos.cpu().context().memory.read(0xb9000, &mut cell).unwrap();
        assert_eq!(cell, [b'K', 0x1e]);
    });
}

#[test]
fn scrolling_moves_only_the_requested_window_and_fills_with_its_attribute() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(PROGRAM, Default::default()).unwrap();
        for (row, text) in [b"1234", b"5678", b"abcd"].into_iter().enumerate() {
            let bytes: Vec<_> = text.iter().flat_map(|&byte| [byte, 0x17]).collect();
            dos.cpu()
                .context()
                .memory
                .write(0xb8000 + row as u32 * 160, &bytes)
                .unwrap();
        }
        video(&mut dos, 0x0601, 0x4e00, 0x0001, 0x0202);
        let screen = dos.screen();
        for (row, expected) in [b"1674", b"5bc8", b"a  d"].into_iter().enumerate() {
            for (column, &character) in expected.iter().enumerate() {
                assert_eq!(screen.cells[row * 80 + column].character, character);
            }
        }
        assert_eq!(screen.cells[161].attribute, 0x4e);
        assert_eq!(screen.cells[160].attribute, 0x17);

        video(&mut dos, 0x0701, 0x5f00, 0x0001, 0x0202);
        let screen = dos.screen();
        for (row, expected) in [b"1  4", b"5678", b"abcd"].into_iter().enumerate() {
            for (column, &character) in expected.iter().enumerate() {
                assert_eq!(screen.cells[row * 80 + column].character, character);
            }
        }
        assert_eq!(screen.cells[1].attribute, 0x5f);
        video(&mut dos, 0x0600, 0x2a00, 0x0001, 0x0202);
        let screen = dos.screen();
        for index in [1, 2, 81, 82, 161, 162] {
            assert_eq!(
                screen.cells[index],
                TextCell {
                    character: b' ',
                    attribute: 0x2a
                }
            );
        }
        assert_eq!(screen.cells[0].character, b'1');
        assert_eq!(screen.cursor.column, 0);
        assert_eq!(screen.cursor.row, 0);
    });
}

#[test]
fn teletype_handles_controls_wraps_and_scrolls_at_the_bottom() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(PROGRAM, Default::default()).unwrap();
        dos.cpu()
            .context()
            .memory
            .write(0xb8f00, &[b' ', 0x2a])
            .unwrap();
        video(&mut dos, 0x0200, 0, 0, 0x184f); // Last visible cell.
        video(&mut dos, 0x0920, 0x001f, 1, 0);
        video(&mut dos, 0x0e5a, 0, 0, 0);
        let screen = dos.screen();
        assert_eq!(
            screen.cells[23 * 80 + 79],
            TextCell {
                character: b'Z',
                attribute: 0x1f
            }
        );
        assert_eq!(
            screen.cells[24 * 80],
            TextCell {
                character: b' ',
                attribute: 0x2a
            }
        );
        assert_eq!(screen.cursor.row, 24);
        assert_eq!(screen.cursor.column, 0);

        video(&mut dos, 0x0200, 0, 0, 0x0203);
        for byte in [b'A', 8, b'B', b'\r', b'\n', b'C', 7] {
            video(&mut dos, 0x0e00 | u16::from(byte), 0, 0, 0);
        }
        let screen = dos.screen();
        assert_eq!(screen.cells[2 * 80 + 3].character, b'B');
        assert_eq!(screen.cells[3 * 80].character, b'C');
        assert_eq!(screen.cursor.row, 3);
        assert_eq!(screen.cursor.column, 1);
    });
}
