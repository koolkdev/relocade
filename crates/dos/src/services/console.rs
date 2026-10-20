use super::{
    buffers::{transfer_count, write_wrapping},
    return_to_host,
};
use crate::{interrupt::return_result, stdio::OutputStream, DosState, ServiceError};
use relocade_runtime::{
    real_mode_interrupt::{return_from_interrupt, set_zero},
    HostCall,
};
use relocade_x86::{ExecutionExit, Segment, StoredSegment};
use std::ops::ControlFlow;

mod line;
use line::{Echo, LineInput};

// DOS's console handle buffer allows 127 input bytes plus CR; reads also return LF.
const CONSOLE_LINE_CAPACITY: u8 = 128;

/// AH=06h: DL=FFh polls stdin without echo; other values write a byte to stdout.
pub(super) fn direct_io(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let byte = call.cpu().dl();
    if byte == 0xff {
        let result = call.state_mut().files.read_character(0);
        let character = match result {
            Ok(character) => character,
            Err(error) => return return_result(call, Err(error)),
        };
        call.cpu().set_al(character.unwrap_or(0));
        set_zero(call, character.is_none());
    } else {
        // Like AH=09h, character output has no write-error return.
        let _ = call.state_mut().files.write_characters(1, &[byte]);
        call.cpu().set_al(byte);
    }
    return_from_interrupt(call)
}

/// AH=07h: read stdin without echo or Ctrl-C processing, waiting if necessary.
pub(super) fn read_character(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let result = call.state_mut().files.read_character(0);
    match result {
        Ok(Some(byte)) => return_character(call, byte),
        Ok(None) => match call.state().files.wait_for_character(0) {
            Ok(input) => call.defer(async move |call| {
                let byte = input.await;
                return_character(call, byte)
            }),
            Err(error) => return_result(call, Err(error)),
        },
        Err(error) => return_result(call, Err(error)),
    }
}

fn return_character(call: &mut HostCall<'_, DosState>, byte: u8) -> ControlFlow<ExecutionExit> {
    call.cpu().set_al(byte);
    return_from_interrupt(call)
}

/// AH=0Ah: edit a line into DS:DX's buffer, whose capacity includes the final CR.
pub(super) fn read_buffered(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let segment = call.cpu().segment(Segment::Ds);
    let offset = call.cpu().dx();
    let mut capacity = [0];
    call.memory()
        .read_segment(&segment, u32::from(offset), &mut capacity)
        .expect("real-mode line buffer");
    if capacity[0] == 0 {
        return return_from_interrupt(call);
    }
    let mut line = LineInput::new(capacity[0], 0, Echo::Stdout);
    match line.read_available(&mut call.state_mut().files) {
        Ok(true) => return_line(call, &segment, offset, &line.bytes),
        Ok(false) => call.defer(async move |call| {
            match line.read_remaining(&mut call.state_mut().files).await {
                Ok(()) => return_line(call, &segment, offset, &line.bytes),
                Err(error) => return_result(call, Err(error)),
            }
        }),
        Err(error) => return_result(call, Err(error)),
    }
}

fn return_line(
    call: &mut HostCall<'_, DosState>,
    segment: &StoredSegment,
    offset: u16,
    bytes: &[u8],
) -> ControlFlow<ExecutionExit> {
    let count = (bytes.len() - 1) as u8; // DOS's count excludes the carriage return.
    let mut memory = call.memory();
    write_wrapping(&mut memory, segment, offset.wrapping_add(1), &[count]);
    write_wrapping(&mut memory, segment, offset.wrapping_add(2), bytes);
    return_from_interrupt(call)
}

/// AH=3Fh on a console input handle: read from one edited line, retaining its remainder.
pub(super) fn read_handle(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let handle = call.cpu().bx();
    if call.state().files.console_eof(handle).get() {
        call.cpu().set_ax(0);
        return return_result(call, Ok(()));
    }
    if call.cpu().cx() == 0 || !call.state().files.stdio.console_line.is_empty() {
        return return_handle_buffer(call);
    }
    let mut line = LineInput::new(CONSOLE_LINE_CAPACITY, handle, Echo::Console);
    match line.read_available(&mut call.state_mut().files) {
        Ok(true) => return_handle_line(call, line.bytes),
        Ok(false) => call.defer(async move |call| {
            match line.read_remaining(&mut call.state_mut().files).await {
                Ok(()) => return_handle_line(call, line.bytes),
                Err(error) => return_result(call, Err(error)),
            }
        }),
        Err(error) => return_result(call, Err(error)),
    }
}

fn return_handle_line(
    call: &mut HostCall<'_, DosState>,
    mut bytes: Vec<u8>,
) -> ControlFlow<ExecutionExit> {
    let handle = call.cpu().bx();
    let eof = bytes.first() == Some(&0x1a);
    call.state().files.console_eof(handle).set(eof);
    let stdio = &mut call.state_mut().files.stdio;
    stdio.output(OutputStream::Stdout).extend(b"\n");
    // Ctrl-Z at the start of a console line reports EOF.
    if !eof {
        bytes.push(b'\n');
        stdio.console_line.extend(bytes);
    }
    return_handle_buffer(call)
}

fn return_handle_buffer(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let cpu = call.cpu();
    let offset = cpu.dx();
    let count = transfer_count(offset, cpu.cx());
    let segment = cpu.segment(Segment::Ds);
    let line = &mut call.state_mut().files.stdio.console_line;
    let count = count.min(line.len());
    let bytes: Vec<_> = line.drain(..count).collect();
    if count != 0 {
        call.memory()
            .write_segment(&segment, u32::from(offset), &bytes)
            .expect("real-mode console read buffer");
    }
    call.cpu().set_ax(count as u16);
    return_result(call, Ok(()))
}

/// AH=09h: output the '$'-terminated string at DS:DX and return AL='$'.
pub(super) fn write_string(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let segment = call.cpu().segment(Segment::Ds);
    let start = call.cpu().dx();
    for index in 0..=u16::MAX {
        let mut byte = [0];
        call.memory()
            .read_segment(&segment, u32::from(start.wrapping_add(index)), &mut byte)
            .expect("real-mode byte address");
        if byte[0] == b'$' {
            call.cpu().set_al(b'$');
            return return_from_interrupt(call);
        }
        // AH=09h writes through stdout, without returning a write-error status.
        let _ = call.state_mut().files.write_characters(1, &byte);
    }
    return_to_host(call, Err(ServiceError::UnterminatedString))
}
