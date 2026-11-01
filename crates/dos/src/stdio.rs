//! Persistent console streams, cooked input buffering and output position.

use relocade_bios::Keyboard;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    io::{self, Read},
    rc::Rc,
};

#[derive(Clone, Copy)]
pub(crate) enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Default)]
pub(crate) struct Stdio {
    pub(crate) input: Keyboard,
    // Unread bytes of a completed AH=3Fh line, shared by all console input handles.
    console_line: RefCell<VecDeque<u8>>,
    // DOS's logical column for cooked output, not the BIOS screen cursor.
    // Escape-sequence bytes count here even when a driver interprets them.
    cooked_column: Cell<u8>,
    stdout: Output,
    stderr: Output,
}

impl Stdio {
    pub(crate) fn new(input: Keyboard) -> Self {
        Self {
            input,
            ..Self::default()
        }
    }

    pub(crate) fn has_buffered_line(&self) -> bool {
        !self.console_line.borrow().is_empty()
    }

    pub(crate) fn buffer_line(&self, bytes: Vec<u8>) {
        self.console_line.borrow_mut().extend(bytes);
    }

    /// Takes up to count bytes from a completed console line.
    pub(crate) fn take_line_bytes(&self, count: usize) -> Vec<u8> {
        let mut line = self.console_line.borrow_mut();
        let count = count.min(line.len());
        line.drain(..count).collect()
    }

    /// Expands tabs and tracks the DOS cooked-output column.
    /// Returns the output byte and how many times it should be written.
    pub(crate) fn cook_character(&self, byte: u8) -> (u8, u8) {
        let column = self.cooked_column.get();
        let (byte, count) = if byte == b'\t' {
            (b' ', 8 - (column & 7))
        } else {
            (byte, 1)
        };
        self.cooked_column.set(match byte {
            b'\r' => 0,
            b'\x08' => column.wrapping_sub(1),
            b' '..=b'~' | 0x80..=0xff => column.wrapping_add(count),
            _ => column,
        });
        (byte, count)
    }

    pub(crate) fn output(&self, stream: OutputStream) -> &Output {
        match stream {
            OutputStream::Stdout => &self.stdout,
            OutputStream::Stderr => &self.stderr,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct Output(Rc<RefCell<VecDeque<u8>>>);

impl Output {
    pub(crate) fn extend(&self, bytes: &[u8]) {
        self.0.borrow_mut().extend(bytes);
    }
}

impl Read for Output {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.0.borrow_mut().read(bytes)
    }
}
