//! Owned host streams sharing the DOS console's byte queues.

use relocade_bios::Keyboard;
use std::{
    cell::RefCell,
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
    pub(crate) console_line: VecDeque<u8>,
    // DOS's cooked-output column, independent of how the host drains its queues.
    pub(crate) output_column: u8,
    stdout: Output,
    stderr: Output,
}

impl Stdio {
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
