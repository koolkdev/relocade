//! Resident ANSI console output using the BIOS page and cursor operations.

mod attributes;
mod parser;

use super::{attributes as driver_attributes, CharacterDevice, CharacterDriver, ReadFuture};
use crate::{Dos, DosError};
use attributes::Attributes;
use parser::{Command, Parameters, Parser};
use relocade_bios::screen::{CursorPosition, Output};
use relocade_machine::MemoryBus;
use std::{cell::RefCell, rc::Rc};

/// A resident ANSI text console. Owns escape, rendition and saved-cursor state;
/// delegates input, input status and flushing to the CON device found at construction.
/// Screen operations share BIOS text memory and its active page and cursor.
/// Unsupported sequences are consumed; mode switching and keyboard remapping
/// are not implemented. Constructing the driver does not install it.
///
/// ```no_run
/// # fn example(dos: &relocade_dos::Dos) -> Result<(), relocade_dos::DosError> {
/// use relocade_dos::devices::AnsiConsole;
/// dos.install_driver(AnsiConsole::new(dos));
/// dos.console().write(b"\x1b[1;36mRelocade\x1b[0m\r\n")?;
/// # Ok(())
/// # }
/// ```
pub struct AnsiConsole {
    previous: Rc<CharacterDevice>,
    memory: MemoryBus,
    state: RefCell<State>,
}

impl AnsiConsole {
    /// Retains the current CON device for input and this session's BIOS memory.
    pub fn new(dos: &Dos) -> Self {
        Self {
            previous: dos.device(b"CON").expect("DOS installs a console device"),
            memory: dos.state.memory().clone(),
            state: RefCell::default(),
        }
    }
}

impl CharacterDriver for AnsiConsole {
    fn name(&self) -> [u8; 8] {
        *b"CON     "
    }
    fn attributes(&self) -> u16 {
        driver_attributes::CONSOLE
    }
    fn input_available(&self) -> bool {
        self.previous.input_available()
    }
    fn flush_input(&self) {
        self.previous.flush_input();
    }
    fn try_read_character(&self) -> Result<Option<u8>, DosError> {
        self.previous.try_read_character()
    }
    fn read_character(&self) -> ReadFuture<'_> {
        self.previous.read_character()
    }
    fn output_ready(&self) -> bool {
        true
    }
    fn write(&self, bytes: &[u8]) -> Result<(), DosError> {
        self.state.borrow_mut().write(&self.memory, bytes);
        Ok(())
    }
    fn output_column(&self) -> Option<u8> {
        Output::active_text(&mut self.memory.view(), self.memory.io())
            .map(|output| output.position().column as u8)
    }
}

#[derive(Default)]
struct State {
    parser: Parser,
    attributes: Attributes,
    saved_cursor: CursorPosition,
}

impl State {
    fn write(&mut self, memory: &MemoryBus, bytes: &[u8]) {
        let mut view = memory.view();
        let mut output = Output::active_text(&mut view, memory.io());
        for &byte in bytes {
            match self.parser.push(byte) {
                Some(Command::Character(byte)) => {
                    if let Some(output) = &mut output {
                        if byte == b'\t' {
                            let count = 8 - (output.position().column & 7);
                            output.write(&[b' '; 8][..usize::from(count)], self.attributes.byte());
                        } else {
                            output.write(&[byte], self.attributes.byte());
                        }
                    }
                }
                Some(Command::Control(b'm', parameters)) => {
                    self.attributes.set(parameters.values());
                }
                Some(Command::Control(command, parameters)) => {
                    if let Some(output) = &mut output {
                        self.control(output, command, &parameters);
                    }
                }
                None => {}
            }
        }
        if let Some(output) = output {
            output.commit_cursor();
        }
    }

    fn control(&mut self, output: &mut Output<'_, '_>, command: u8, parameters: &Parameters) {
        let position = output.position();
        let distance = parameters.get(0, 1);
        let mut cursor = position;
        match command {
            b'A' => cursor.row = cursor.row.saturating_sub(distance),
            b'B' => cursor.row = cursor.row.saturating_add(distance),
            b'C' => cursor.column = cursor.column.saturating_add(distance),
            b'D' => cursor.column = cursor.column.saturating_sub(distance),
            b'H' | b'f' => {
                cursor.row = parameters.get(0, 1) - 1;
                cursor.column = parameters.get(1, 1) - 1;
            }
            b's' => self.saved_cursor = position,
            b'u' => cursor = self.saved_cursor,
            b'J' => {
                self.erase_display(output, parameters.get(0, 0));
                // DOS ANSI's clear-screen command also homes the cursor.
                if parameters.get(0, 0) == 2 {
                    cursor = CursorPosition::default();
                }
            }
            b'K' => self.erase_line(output, parameters.get(0, 0)),
            _ => return,
        }
        if cursor != position {
            output.move_to(cursor);
        }
    }

    fn erase_line(&self, output: &mut Output<'_, '_>, mode: u16) {
        let cursor = output.position();
        let (left, right) = match mode {
            0 => (cursor.column, output.columns() - 1),
            1 => (0, cursor.column),
            2 => (0, output.columns() - 1),
            _ => return,
        };
        output.clear(
            CursorPosition {
                row: cursor.row,
                column: left,
            },
            CursorPosition {
                row: cursor.row,
                column: right,
            },
            self.attributes.byte(),
        );
    }

    fn erase_display(&self, output: &mut Output<'_, '_>, mode: u16) {
        let cursor = output.position();
        let end = CursorPosition {
            row: output.rows() - 1,
            column: output.columns() - 1,
        };
        match mode {
            0 => {
                self.erase_line(output, 0);
                if cursor.row < end.row {
                    output.clear(
                        CursorPosition {
                            row: cursor.row + 1,
                            column: 0,
                        },
                        end,
                        self.attributes.byte(),
                    );
                }
            }
            1 => {
                if cursor.row != 0 {
                    output.clear(
                        CursorPosition::default(),
                        CursorPosition {
                            row: cursor.row - 1,
                            column: end.column,
                        },
                        self.attributes.byte(),
                    );
                }
                self.erase_line(output, 1);
            }
            2 => output.clear(CursorPosition::default(), end, self.attributes.byte()),
            _ => {}
        }
    }
}
