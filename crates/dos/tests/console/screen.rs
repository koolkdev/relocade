use super::{common, path_service, service, success, SERVICE_PROGRAM};
use relocade_bios::TextCell;
use relocade_x86::ExecutionExit;

#[test]
fn dos_output_uses_the_bios_page_cursor_and_guest_cell_attributes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(
            &[
                0xb8, 1, 5, 0xcd, 0x10, // INT 10h/AH=05h: select page 1.
                0xb4, 2, 0xb7, 1, 0xba, 2, 1, 0xcd, 0x10, // Cursor at row 1, column 2.
                0xb8, 0x00, 0xb9, 0x8e, 0xc0, // ES = B900 (text page 1).
                0x26, 0xc7, 0x06, 0xa4, 0, b'X', 0x1e, // ES:[164] = yellow-on-blue X.
                0xb4, 9, 0xba, 0x00, 3, 0xcd, 0x21, // DOS writes DS:0300's string.
                0xb8, b'!', 0x0e, 0xcd, 0x10, // BIOS teletype follows DOS's cursor.
                0xb8, 0, 0x4c, 0xcd, 0x21,
            ],
            Default::default(),
        )
        .unwrap();
        dos.runtime()
            .machine()
            .memory()
            .write(0x10300, b"DOS$")
            .unwrap();
        assert_eq!(dos.runtime().run(100).unwrap(), ExecutionExit::HostReturn);
        assert_eq!(dos.exit_status(), Some(Ok(0)));
        let screen = dos.screen();
        assert_eq!(screen.page, 1);
        assert_eq!(
            screen.cells[82],
            TextCell {
                character: b'D',
                attribute: 0x1e,
            }
        );
        assert_eq!(
            [83, 84, 85].map(|index| screen.cells[index].character),
            *b"OS!"
        );
        assert_eq!((screen.cursor.row, screen.cursor.column), (1, 6));
        assert_eq!(common::output(dos.stdout()), b"DOS"); // BIOS output is screen-only.
        let mut page_zero = [0; 2];
        dos.runtime()
            .machine()
            .memory()
            .read(0xb80a4, &mut page_zero)
            .unwrap();
        assert_eq!(page_zero, [b' ', 7]);
    });
}

#[test]
fn file_redirection_skips_the_screen_but_stderr_and_opened_con_still_render() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let file = success(&path_service(&mut dos, b"OUT.TXT", 0x3c00, 0));
        success(&service(&mut dos, 0x4600, file, 1, 0));
        let blank = dos.screen();
        dos.runtime()
            .machine()
            .memory()
            .write(0x10300, b"File$")
            .unwrap();
        service(&mut dos, 0x0900, 0, 0, 0x300);
        success(&service(&mut dos, 0x4000, 1, 4, 0x300));
        assert_eq!(dos.read_file("OUT.TXT").unwrap(), b"FileFile");
        assert_eq!(dos.screen(), blank);
        assert!(common::output(dos.stdout()).is_empty());

        let console = success(&path_service(&mut dos, b"CON", 0x3d02, 0));
        dos.runtime()
            .machine()
            .memory()
            .write(0x10300, b"E\r\nC")
            .unwrap();
        success(&service(&mut dos, 0x4000, 2, 3, 0x300));
        success(&service(&mut dos, 0x4000, console, 1, 0x303));
        assert_eq!(common::output(dos.stderr()), b"E\r\n");
        assert_eq!(common::output(dos.stdout()), b"C");

        // Direct AH=06h output also follows the restored CON handle.
        success(&service(&mut dos, 0x4600, console, 1, 0));
        service(&mut dos, 0x0600, 0, 0, u16::from(b'D'));
        // Raw AH=40h keeps control bytes that a cooked handle would stop at.
        success(&service(&mut dos, 0x4401, console, 0, 0x23));
        dos.runtime()
            .machine()
            .memory()
            .write(0x10300, b"\x1aR")
            .unwrap();
        assert_eq!(success(&service(&mut dos, 0x4000, console, 2, 0x300)), 2);
        let screen = dos.screen();
        assert_eq!(screen.cells[0].character, b'E');
        assert_eq!(
            [80, 81, 82, 83].map(|index| screen.cells[index].character),
            *b"CD\x1aR"
        );
        assert_eq!((screen.cursor.row, screen.cursor.column), (1, 4));
        assert_eq!(common::output(dos.stdout()), b"D\x1aR");
    });
}
