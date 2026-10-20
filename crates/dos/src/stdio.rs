//! Buffered host streams, accessed between guest runs.

use crate::DosState;
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    ops::DerefMut,
};

#[derive(Clone, Copy)]
pub(crate) enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Default)]
pub(crate) struct Stdio {
    pub(crate) input: VecDeque<u8>,
    stdout: VecDeque<u8>,
    stderr: VecDeque<u8>,
}

impl Stdio {
    pub(crate) fn output(&mut self, stream: OutputStream) -> &mut VecDeque<u8> {
        match stream {
            OutputStream::Stdout => &mut self.stdout,
            OutputStream::Stderr => &mut self.stderr,
        }
    }
}

pub(crate) struct Stdin<S>(pub(crate) S);

impl<S: DerefMut<Target = DosState>> Write for Stdin<S> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.files.stdio.input.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) struct Output<S> {
    pub(crate) state: S,
    pub(crate) stream: OutputStream,
}

impl<S: DerefMut<Target = DosState>> Read for Output<S> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.state.files.stdio.output(self.stream).read(bytes)
    }
}
