//! Cooked console policy for DOS handle reads and writes.
//!
//! Raw device transfers bypass this module. Line editing, echo, buffered line
//! remainders, Ctrl-Z and tab expansion belong to DOS, not to character drivers.

use super::input::read_line;
use crate::{
    console::CharacterOutput,
    error::DosError,
    files::{DeviceHandle, Handle},
    stdio::Stdio,
};
use std::rc::Rc;

// DOS's console handle buffer allows 127 input bytes plus CR; reads also return LF.
const LINE_CAPACITY: u8 = 128;

pub(in crate::services) async fn read(
    stdio: &Stdio,
    handle: &Rc<DeviceHandle>,
    count: usize,
) -> Result<Vec<u8>, DosError> {
    if !stdio.has_buffered_line() {
        let source = Handle::Device(handle.clone());
        let output = CharacterOutput::handle(stdio, Ok(source.clone()));
        let mut bytes = read_line(&source, LINE_CAPACITY, &output).await?;
        let eof = bytes.first() == Some(&0x1a);
        handle.set_eof(eof);
        output.write_all(b"\n").await?;
        if !eof {
            bytes.push(b'\n');
            stdio.buffer_line(bytes);
        }
    }
    Ok(stdio.take_line_bytes(count))
}

pub(in crate::services) async fn write(
    stdio: &Stdio,
    handle: &Rc<DeviceHandle>,
    bytes: &[u8],
) -> Result<usize, DosError> {
    handle.set_eof(false);
    // Cooked writes stop before Ctrl-Z and count source bytes, including tabs.
    let count = bytes
        .iter()
        .position(|&byte| byte == 0x1a)
        .unwrap_or(bytes.len());
    let output = CharacterOutput::handle(stdio, Ok(Handle::Device(handle.clone())));
    for &byte in &bytes[..count] {
        output.write_cooked(byte).await?;
    }
    Ok(count)
}
