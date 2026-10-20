//! Owned host streams sharing the DOS console's byte queues.

use std::{
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    io::{self, Read, Write},
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[derive(Clone, Copy)]
pub(crate) enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Default)]
pub(crate) struct Stdio {
    pub(crate) input: Stdin,
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

    pub(crate) fn clear(&mut self) {
        *self.input.0.borrow_mut() = Input::default();
        self.console_line.clear();
        self.output_column = 0;
        self.stdout.0.borrow_mut().clear();
        self.stderr.0.borrow_mut().clear();
    }
}

#[derive(Default)]
struct Input {
    bytes: VecDeque<u8>,
    waker: Option<Waker>,
}

#[derive(Clone, Default)]
pub(crate) struct Stdin(Rc<RefCell<Input>>);

impl Stdin {
    pub(crate) fn has_input(&self) -> bool {
        !self.0.borrow().bytes.is_empty()
    }

    pub(crate) fn discard_queued(&self) {
        self.0.borrow_mut().bytes.clear();
    }

    pub(crate) fn try_read(&self) -> Option<u8> {
        self.0.borrow_mut().bytes.pop_front()
    }

    pub(crate) fn read_character(&self) -> ReadCharacter {
        ReadCharacter {
            input: self.clone(),
        }
    }
}

impl Write for Stdin {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !bytes.is_empty() {
            let waker = {
                let mut input = self.0.borrow_mut();
                input.bytes.extend(bytes);
                input.waker.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// One guest can have one pending character read. Dropping it releases its waker.
pub(crate) struct ReadCharacter {
    input: Stdin,
}

impl Future for ReadCharacter {
    type Output = u8;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u8> {
        let mut input = self.input.0.borrow_mut();
        match input.bytes.pop_front() {
            Some(byte) => Poll::Ready(byte),
            None => {
                input.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl Drop for ReadCharacter {
    fn drop(&mut self) {
        self.input.0.borrow_mut().waker = None;
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
