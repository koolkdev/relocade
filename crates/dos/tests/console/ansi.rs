use super::{common, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::{
    devices::{AnsiConsole, CharacterDriver, DriverFuture},
    Dos,
};
use std::{cell::Cell, io::Write, rc::Rc};

fn write_handle(dos: &mut Dos, handle: u16, bytes: &[u8]) {
    dos.cpu().context().memory.write(0x10400, bytes).unwrap();
    assert_eq!(
        success(&service(dos, 0x4000, handle, bytes.len() as u16, 0x400)),
        bytes.len() as u16
    );
}

fn cell(dos: &mut Dos, row: u32, column: u32) -> [u8; 2] {
    let mut cell = [0; 2];
    dos.cpu()
        .context()
        .memory
        .read(0xb8000 + (row * 80 + column) * 2, &mut cell)
        .unwrap();
    cell
}

fn cursor(dos: &mut Dos) -> u16 {
    dos.cpu().context().memory.read_u16(0x450).unwrap()
}

#[test]
fn ansi_state_is_shared_by_rust_character_string_and_handle_output_across_launches() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.install_driver(AnsiConsole::new(&dos));
        dos.console().write(b"\x1b[1;").await.unwrap();
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        for byte in b"31" {
            service(&mut dos, 0x0200, 0, 0, u16::from(*byte));
        }
        dos.cpu().context().memory.write(0x10300, b"mR$").unwrap();
        service(&mut dos, 0x0900, 0, 0, 0x300);
        write_handle(&mut dos, 2, b"\x1b[44mB");
        common::terminate(&mut dos);
        let exe = [0xb4, 2, 0xb2, b'Y', 0xcd, 0x21, 0xb8, 0, 0x4c, 0xcd, 0x21];
        dos.launch_mz(&common::mz_image(&exe), Default::default())
            .unwrap();
        assert_eq!(
            dos.cpu().run(100).await,
            relocade_x86::ExecutionExit::Host(relocade_x86::HostExit::Return)
        );
        dos.console().write(b"Z\x1b[0mN").await.unwrap();
        assert_eq!(cell(&mut dos, 0, 0), [b'R', 0x0c]);
        assert_eq!(cell(&mut dos, 0, 1), [b'B', 0x1c]);
        assert_eq!(cell(&mut dos, 0, 2), [b'Y', 0x1c]);
        assert_eq!(cell(&mut dos, 0, 3), [b'Z', 0x1c]);
        assert_eq!(cell(&mut dos, 0, 4), [b'N', 0x07]);
        assert_eq!(common::output(dos.stdout()), b"31mRY");
        assert_eq!(common::output(dos.stderr()), b"\x1b[44mB");
    });
}

#[test]
fn ansi_installation_changes_new_opens_and_leaves_file_redirection_literal() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let old = success(&path_service(&mut dos, b"CON", 0x3d02, 0));
        let old_duplicate = success(&service(&mut dos, 0x4500, old, 0, 0));
        dos.install_driver(AnsiConsole::new(&dos));
        write_handle(&mut dos, old_duplicate, b"\x1b[31mO");
        assert_eq!(cell(&mut dos, 0, 0), [0x1b, 7]);
        assert_eq!(cell(&mut dos, 0, 5), [b'O', 7]);
        let new = success(&path_service(&mut dos, b"con.txt", 0x3d02, 0));
        write_handle(&mut dos, new, b"\x1b[2J\x1b[32mN");
        assert_eq!(cell(&mut dos, 0, 0), [b'N', 2]);
        write_handle(&mut dos, 1, b"X"); // The existing standard handle still uses plain CON.
        assert_eq!(cell(&mut dos, 0, 1), [b'X', 7]);
        success(&service(&mut dos, 0x4600, new, 1, 0));
        write_handle(&mut dos, 1, b"G");
        assert_eq!(cell(&mut dos, 0, 2), [b'G', 2]);

        let file = success(&path_service(&mut dos, b"OUT.TXT", 0x3c00, 0));
        success(&service(&mut dos, 0x4600, file, 1, 0));
        let bytes = b"\x1b[31mred\x1b[0m";
        write_handle(&mut dos, 1, bytes);
        assert_eq!(dos.read_file("OUT.TXT").unwrap(), bytes);
        assert_eq!(cursor(&mut dos), 3);
        dos.console().write(b"R").await.unwrap(); // Redirected SGR did not touch the driver.
        assert_eq!(cell(&mut dos, 0, 3), [b'R', 2]);
    });
}

#[test]
fn ansi_reads_bios_input_while_the_rust_console_edits_and_echoes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.install_driver(AnsiConsole::new(&dos));
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        assert_eq!(success(&service(&mut dos, 0x4406, 0, 0, 0)) as u8, 0);
        dos.stdin().write_all(b"old").unwrap();
        assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0xff);
        let cpu = service(&mut dos, 0x0c06, 0, 0, 0xff);
        assert_ne!(cpu.flags.eflags() & 0x40, 0); // Flushed, then direct read sees no input.
        let console = dos.console();
        let mut line = Box::pin(console.read_line());
        common::assert_pending(line.as_mut());
        // Output can use the driver while its BIOS keyboard read is awaiting input.
        dos.console().write(b"\x1b[32m> ").await.unwrap();
        dos.stdin().write_all(b"ax\x08b\r").unwrap();
        assert_eq!(line.await.unwrap(), b"ab");
        assert_eq!(cell(&mut dos, 0, 2), [b'a', 2]);
        assert_eq!(cell(&mut dos, 0, 3), [b'b', 2]);
        assert_eq!(cursor(&mut dos), 0x0100);
        assert!(common::output(dos.stdout()).is_empty());
    });
}

#[test]
fn ansi_colors_controls_tabs_and_scroll_use_the_bios_grid() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.install_driver(AnsiConsole::new(&dos));
        for (index, attribute) in [0, 4, 2, 6, 1, 5, 3, 7].into_iter().enumerate() {
            dos.console()
                .write(format!("\x1b[{}mX", 30 + index).as_bytes())
                .await
                .unwrap();
            assert_eq!(cell(&mut dos, 0, index as u32), [b'X', attribute]);
        }
        dos.console()
            .write(b"\x1b[0;31;44;1;5mA\x1b[7mB\x1b[8mC\x1b[mD")
            .await
            .unwrap();
        assert_eq!(cell(&mut dos, 0, 8), [b'A', 0x9c]);
        assert_eq!(cell(&mut dos, 0, 9), [b'B', 0xc9]);
        assert_eq!(cell(&mut dos, 0, 10), [b'C', 0xc4]);
        assert_eq!(cell(&mut dos, 0, 11), [b'D', 7]);

        dos.device(b"CON")
            .unwrap()
            .write(b"\x1b[2J\x1b[31mabc\tT\x1b[2;4H\tU")
            .await
            .unwrap();
        assert_eq!(cell(&mut dos, 0, 8), [b'T', 4]);
        assert_eq!(cell(&mut dos, 1, 8), [b'U', 4]);
        dos.console()
            .write(b"\x1b[3;5H\x1b[s\x1b[2B\x1b[3CA\x1b[A\x1b[2DB\x1b[uC")
            .await
            .unwrap();
        assert_eq!(cell(&mut dos, 4, 7), [b'A', 4]);
        assert_eq!(cell(&mut dos, 3, 6), [b'B', 4]);
        assert_eq!(cell(&mut dos, 2, 4), [b'C', 4]);
        dos.console().write(b"\x1b[999;999H").await.unwrap();
        assert_eq!(cursor(&mut dos), 0x184f);
        dos.console().write(b"\x1b[999A\x1b[999D").await.unwrap();
        assert_eq!(cursor(&mut dos), 0);

        dos.console()
            .write(b"\x1b[33;44m\x1b[25;80HZ")
            .await
            .unwrap();
        assert_eq!(cell(&mut dos, 23, 79), [b'Z', 0x16]);
        for column in 0..80 {
            assert_eq!(cell(&mut dos, 24, column), [b' ', 0x16]);
        }
        assert_eq!(cursor(&mut dos), 0x1800);
    });
}

#[test]
fn ansi_erasing_uses_current_attributes_and_preserves_the_cursor_except_clear_screen() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.install_driver(AnsiConsole::new(&dos));
        dos.console()
            .write(b"ABCDEFG\r\nABCDEFG\r\nABCDEFG\x1b[31;44m\x1b[2;4H\x1b[K")
            .await
            .unwrap();
        assert_eq!(cursor(&mut dos), 0x0103);
        assert_eq!(cell(&mut dos, 1, 2), [b'C', 7]);
        assert_eq!(cell(&mut dos, 1, 3), [b' ', 0x14]);
        assert_eq!(cell(&mut dos, 2, 3), [b'D', 7]);
        dos.console().write(b"\x1b[1J").await.unwrap();
        assert_eq!(cell(&mut dos, 0, 0), [b' ', 0x14]);
        assert_eq!(cell(&mut dos, 1, 2), [b' ', 0x14]);
        assert_eq!(cell(&mut dos, 2, 0), [b'A', 7]);
        dos.console().write(b"\x1b[J").await.unwrap();
        assert_eq!(cell(&mut dos, 2, 0), [b' ', 0x14]);
        assert_eq!(cell(&mut dos, 24, 79), [b' ', 0x14]);
        assert_eq!(cursor(&mut dos), 0x0103);
        dos.console().write(b"\x1b[2J").await.unwrap();
        assert_eq!(cursor(&mut dos), 0);
    });
}

#[test]
fn ansi_parser_recovers_from_unsupported_overflowed_and_cancelled_sequences() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.install_driver(AnsiConsole::new(&dos));
        for sequence in [
            b"\x1b[999999999999999999999999m".as_slice(),
            b"\x1b[1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;31m",
            b"\x1b[?25h",
            b"\x1b[31\x18",
            b"\x1b[31\x1b[m",
        ] {
            dos.console().write(sequence).await.unwrap();
            dos.console().write(b"X").await.unwrap();
        }
        for column in 0..5 {
            assert_eq!(cell(&mut dos, 0, column), [b'X', 7]);
        }
        assert_eq!(cursor(&mut dos), 5);
    });
}

#[test]
fn ansi_uses_the_bios_selected_page_and_cursor_without_touching_other_pages() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.install_driver(AnsiConsole::new(&dos));
        let program = [
            0xb8, 1, 5, 0xcd, 0x10, // Select BIOS page 1.
            0xb4, 2, 0xb7, 1, 0xba, 2, 1, 0xcd, 0x10, // Cursor at row 1, column 2.
            0xb8, b'B', 0x0e, 0xcd, 0x10, // BIOS writes B and advances its cursor.
            0xb8, 0, 0x4c, 0xcd, 0x21,
        ];
        dos.launch_com(&program, Default::default()).unwrap();
        assert_eq!(
            dos.cpu().run(100).await,
            relocade_x86::ExecutionExit::Host(relocade_x86::HostExit::Return)
        );
        dos.device(b"CON")
            .unwrap()
            .write(b"\x1b[31mA\tT")
            .await
            .unwrap();
        let memory = dos.cpu().context().memory;
        let mut cells = [0; 14];
        memory.read(0xb9000 + 80 * 2 + 2 * 2, &mut cells).unwrap();
        assert_eq!(&cells[..4], &[b'B', 7, b'A', 4]);
        assert_eq!(&cells[12..], &[b'T', 4]);
        assert_eq!(memory.read_u16(0x452).unwrap(), 0x0109);
        assert_eq!(memory.read_u16(0xb8000 + 80 * 2 + 2 * 2).unwrap(), 0x0720);
    });
}

#[test]
fn ansi_keeps_parsing_when_graphics_or_an_empty_grid_prevents_drawing() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.install_driver(AnsiConsole::new(&dos));
        dos.console().write(b"\x1b[").await.unwrap();
        let mut mode_program = [0xb8, 0x13, 0, 0xcd, 0x10, 0xb8, 0, 0x4c, 0xcd, 0x21];
        dos.launch_com(&mode_program, Default::default()).unwrap();
        dos.cpu().run(100).await;
        dos.cpu()
            .context()
            .memory
            .write(0xa0000, &[0x55; 16])
            .unwrap();
        dos.console()
            .write(b"31;44;5mX\x1b[2J\x1b[25m")
            .await
            .unwrap();
        let mut pixels = [0; 16];
        dos.cpu()
            .context()
            .memory
            .read(0xa0000, &mut pixels)
            .unwrap();
        assert_eq!(pixels, [0x55; 16]);
        mode_program[1] = 3;
        dos.launch_com(&mode_program, Default::default()).unwrap();
        dos.cpu().run(100).await;
        dos.console().write(b"X\x1b[32;45").await.unwrap();
        assert_eq!(cell(&mut dos, 0, 0), [b'X', 0x14]);
        dos.cpu().context().memory.write_u16(0x44a, 0).unwrap(); // BIOS columns unavailable.
        dos.console().write(b"mignored").await.unwrap();
        dos.cpu().context().memory.write_u16(0x44a, 80).unwrap();
        dos.console().write(b"Y").await.unwrap();
        assert_eq!(cell(&mut dos, 0, 1), [b'Y', 0x52]);
        assert_eq!(cursor(&mut dos), 2);
    });
}

struct PreviousConsole(Rc<Cell<bool>>);

impl CharacterDriver for PreviousConsole {
    fn name(&self) -> [u8; 8] {
        *b"CON     "
    }
    fn attributes(&self) -> u16 {
        relocade_dos::devices::attributes::CONSOLE
    }
    fn input_available(&self) -> DriverFuture<'_, bool> {
        Box::pin(async move { Ok(true) })
    }
    fn flush_input(&self) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            self.0.set(true);
            Ok(())
        })
    }
    fn peek_character(&self) -> DriverFuture<'_, Option<u8>> {
        Box::pin(async move { Ok(Some(b'R')) })
    }
    fn read<'a>(&'a self, bytes: &'a mut [u8]) -> DriverFuture<'a, usize> {
        Box::pin(async move {
            bytes.fill(b'R');
            Ok(bytes.len())
        })
    }
}

#[test]
fn ansi_owns_bios_input_without_forwarding_to_the_previous_console() {
    pollster::block_on(async {
        let dos = common::dos().await;
        let bios_console = dos.device(b"CON").unwrap();
        let flushed = Rc::new(Cell::new(false));
        dos.install_driver(PreviousConsole(flushed.clone()));
        let previous = dos.device(b"CON").unwrap();
        dos.install_driver(AnsiConsole::new(&dos));
        let ansi = dos.device(b"CON").unwrap();
        assert!(!ansi.input_available().await.unwrap());
        assert!(previous.input_available().await.unwrap());
        assert_eq!(ansi.peek_character().await.unwrap(), None);
        dos.stdin().write_all(b"K").unwrap();
        assert!(ansi.input_available().await.unwrap());
        for _ in 0..2 {
            assert_eq!(ansi.peek_character().await.unwrap(), Some(b'K'));
        }
        assert_eq!(ansi.read_character().await.unwrap(), b'K');
        assert_eq!(bios_console.peek_character().await.unwrap(), None);

        dos.keyboard().push(relocade_bios::KeyStroke {
            character: 0,
            scan_code: 0x4b,
        });
        dos.stdin().write_all(b"Z").unwrap();
        assert_eq!(ansi.read_character().await.unwrap(), 0);
        assert_eq!(ansi.peek_character().await.unwrap(), Some(0x4b));
        ansi.flush_input().await.unwrap();
        assert!(!ansi.input_available().await.unwrap());
        assert_eq!(bios_console.peek_character().await.unwrap(), None);
        assert!(!flushed.get());
        assert_eq!(previous.read_character().await.unwrap(), b'R');
    });
}

#[test]
fn dos_cooked_tabs_track_output_bytes_while_raw_ansi_tabs_use_the_screen_cursor() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.install_driver(AnsiConsole::new(&dos));
        dos.console().write(b"\r\x1b[31mA\tT").await.unwrap();
        // DOS counted '[31mA' as five columns: TAB becomes three spaces.
        // ANSI displayed only A before those spaces, so T lands at column four.
        assert_eq!(cell(&mut dos, 0, 0), [b'A', 4]);
        for column in 1..4 {
            assert_eq!(cell(&mut dos, 0, column), [b' ', 4]);
        }
        assert_eq!(cell(&mut dos, 0, 4), [b'T', 4]);
        dos.console().write(b"\r\x1b[2;4H\tU").await.unwrap();
        // '[2;4H' also counts as five DOS columns, irrespective of its cursor move.
        assert_eq!(cell(&mut dos, 1, 6), [b'U', 4]);
        assert_eq!(cursor(&mut dos), 0x0107);

        dos.device(b"CON")
            .unwrap()
            .write(b"\x1b[3;4H\tV")
            .await
            .unwrap();
        assert_eq!(cell(&mut dos, 2, 8), [b'V', 4]);
        assert_eq!(cursor(&mut dos), 0x0209);
    });
}
